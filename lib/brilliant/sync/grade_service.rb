module Brilliant
  module Sync
    class GradeService < BaseService
      def sync(course_id, grade_values)
        items = client.ensure_array(grade_values)
        
        # Fetch grade definitions to catch "ungraded" items and extra credit status
        definitions_raw = client.get_grade_definitions(course_id)
        definitions = client.ensure_array(definitions_raw)
        
        if definitions.empty? && items.any?
          puts "[Sync::GradeService] Warning: No grade definitions found for course #{course_id}, but values exist. Syncing from values only."
        elsif definitions.empty? && items.empty?
          puts "[Sync::GradeService] Warning: No definitions OR values found for course #{course_id}."
          # Fallback: Try to find grade items referenced by assignments
          assignment_grade_ids = Assignment.where(course_id: course_id.to_s).where.not(grade_item_id: nil).pluck(:grade_item_id).uniq
          if assignment_grade_ids.any?
            puts "[Sync::GradeService] Fallback: Found #{assignment_grade_ids.size} grade IDs from assignments."
            # We don't have names or max points, but we can at least create the shell records
            # so they appear in the UI.
            assignment_grade_ids.each do |gid|
               next if definitions.any? { |d| d['Identifier'].to_s == gid.to_s }
               definitions << { 'Identifier' => gid, 'Name' => "Grade Item #{gid} (from Assignment)" }
            end
          end
        end
        
        puts "[Sync::GradeService] Course #{course_id}: Found #{items.size} values and #{definitions.size} definitions"

        # Map values by their grade object ID for quick matching
        values_map = items.each_with_object({}) do |v, h|
          id = (v['GradeObjectIdentifier'] || v['Identifier']).to_s
          h[id] = v
        end

        # Map definitions by their ID
        definitions_map = definitions.each_with_object({}) do |d, h|
          id = d['Identifier'].to_s
          h[id] = d
        end

        # Combine all IDs from both sources
        all_ids = (definitions_map.keys + values_map.keys).uniq
        if all_ids.empty?
          puts "[Sync::GradeService] Course #{course_id}: No grade items found in either definitions or values."
          return 
        end

        grades_to_upsert = all_ids.map do |obj_id|
          defn = definitions_map[obj_id]
          val = values_map[obj_id]
          
          name = defn&.dig('Name') || val&.dig('GradeObjectName') || val&.dig('Name') || "Grade Item #{obj_id}"
          
          # Try to find a matching assignment to inherit a due date
          assignment = Assignment.find_by(course_id: course_id.to_s, grade_item_id: obj_id)
          assignment ||= Assignment.where(course_id: course_id.to_s).where("name LIKE ?", "%#{name}%").first
          due_date = assignment&.due_date

          # Determine the denominator
          # Prefer the one from the value if it exists, else the max points from definition
          # D2L API uses PointsDenominator or Denominator
          denominator = val&.dig('GradeValue', 'PointsDenominator') || val&.dig('GradeValue', 'Denominator') || 
                        val&.dig('PointsDenominator') || val&.dig('Denominator') || 
                        defn&.dig('MaxPoints')

          # Determine the numerator
          numerator = val&.dig('GradeValue', 'PointsNumerator') || val&.dig('GradeValue', 'Numerator') || 
                      val&.dig('PointsNumerator') || val&.dig('Numerator')

          # Determine weight
          weight = defn&.dig('Weight') || val&.dig('Weight') || val&.dig('WeightAchieved') || val&.dig('GradeValue', 'Weight')

          {
            course_id: course_id.to_s,
            brightspace_id: obj_id,
            name: name,
            displayed_grade: val&.dig('DisplayedGrade'),
            numerator: numerator,
            denominator: denominator,
            weight: weight,
            is_extra_credit: defn&.dig('IsExtraCredit') || defn&.dig('IsBonus') || false,
            due_date: due_date,
            grade_object_type: defn&.dig('GradeType') || val&.dig('GradeObjectTypeName'),
            updated_at: Time.current,
            created_at: Time.current
          }
        end

        begin
          result = Grade.upsert_all(grades_to_upsert, unique_by: [:course_id, :brightspace_id])
          puts "[Sync::GradeService] Course #{course_id}: Upserted #{grades_to_upsert.size} grade items."
          
          # If an item was manually marked as ungraded, but now has a real grade (> 0),
          # clear the manual flag.
          Grade.where(course_id: course_id.to_s, manually_marked_ungraded: true).each do |g|
            if (g.numerator && g.numerator > 0) || (g.effective_numerator && g.effective_numerator > 0)
              g.update(manually_marked_ungraded: false, updated_at: Time.current)
            end
          end
        rescue => e
          puts "[Sync::GradeService] Grade batch upsert failed for course #{course_id}: #{e.message}"
        end
      end
    end
  end
end
