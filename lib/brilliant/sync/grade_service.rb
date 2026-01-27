module Brilliant
  module Sync
    class GradeService < BaseService
      def sync(course_id, grade_values)
        items = client.ensure_array(grade_values)
        return if items.empty?

        grades_to_upsert = items.map do |g|
          obj_id = (g['GradeObjectIdentifier'] || g['Identifier']).to_s
          {
            course_id: course_id.to_s,
            brightspace_id: obj_id,
            name: g['GradeObjectName'] || "Grade Item #{obj_id}",
            displayed_grade: g['DisplayedGrade'],
            numerator: (g.dig('GradeValue', 'Numerator') || g['Numerator']),
            denominator: (g.dig('GradeValue', 'Denominator') || g['Denominator']),
            updated_at: Time.current,
            created_at: Time.current
          }
        end

        begin
          Grade.upsert_all(grades_to_upsert, unique_by: [:course_id, :brightspace_id])
        rescue => e
          puts "[Sync::GradeService] Grade batch upsert failed: #{e.message}"
        end
      end
    end
  end
end
