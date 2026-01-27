module Brilliant
  module Sync
    class AssignmentService < BaseService
      def sync_dropbox(course_id, assignments)
        items = client.ensure_array(assignments)
        skipped = []
        
        assignments_to_upsert = items.map do |a_summary|
          next if a_summary.nil?
          t_id = (a_summary['Id'] || a_summary['Identifier'] || a_summary['TopicId']).to_s
          
          existing = Assignment.find_by(brightspace_id: t_id, course_id: course_id.to_s)
          if existing&.manually_edited?
            skipped << existing
            next
          end

          desc_obj = a_summary['CustomInstructions'] || a_summary['Description']
          atts = (a_summary['Attachments'] || []) + (a_summary['LinkAttachments'] || [])
          
          {
            course_id: course_id.to_s,
            brightspace_id: t_id,
            name: a_summary['Name'],
            due_date: (Time.zone.parse(a_summary['DueDate']) rescue nil),
            description: html_to_markdown(desc_obj),
            attachments: atts.any? ? atts.to_json : nil,
            is_graded: a_summary['IsGraded'] || false,
            grade_item_id: a_summary['GradeItemId'].to_s,
            assignment_type: 'dropbox',
            updated_at: Time.current,
            created_at: existing&.created_at || Time.current
          }
        end.compact

        begin
          Assignment.upsert_all(assignments_to_upsert, unique_by: :index_assignments_on_course_and_bs_id) if assignments_to_upsert.any?
        rescue => e
          puts "[Sync::AssignmentService] Dropbox batch upsert failed: #{e.message}"
        end

        skipped
      end

      def sync_quizzes(course_id, quizzes)
        items = client.ensure_array(quizzes)
        skipped = []
        
        quizzes_to_upsert = items.map do |q|
          next if q.nil?
          q_id = (q['QuizId'] || q['Id'] || q['Identifier']).to_s
          
          existing = Assignment.find_by(brightspace_id: "quiz_#{q_id}", course_id: course_id.to_s)
          if existing&.manually_edited?
            skipped << existing
            next
          end

          desc_text = html_to_markdown(q['Description'] || q['Header'])
          inst_content = html_to_markdown(q['Instructions'])
          desc_text += "\n\n### Instructions\n#{inst_content}" if inst_content.present?
          
          {
            course_id: course_id.to_s,
            brightspace_id: "quiz_#{q_id}",
            name: q['Name'],
            assignment_type: 'quiz',
            due_date: (Time.zone.parse(q['DueDate']) rescue nil),
            description: desc_text,
            updated_at: Time.current,
            created_at: existing&.created_at || Time.current
          }
        end.compact

        begin
          Assignment.upsert_all(quizzes_to_upsert, unique_by: :index_assignments_on_course_and_bs_id) if quizzes_to_upsert.any?
        rescue => e
          puts "[Sync::AssignmentService] Quiz batch upsert failed: #{e.message}"
        end

        skipped
      end
    end
  end
end
