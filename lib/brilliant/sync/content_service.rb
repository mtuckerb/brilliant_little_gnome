module Brilliant
  module Sync
    class ContentService < BaseService
      def sync(course_id, toc)
        modules_data = toc.is_a?(Hash) ? (toc['Modules'] || []) : toc
        return unless modules_data.is_a?(Array)
        
        @all_modules_to_upsert = []
        @all_items_to_upsert = []
        @course_id = course_id.to_s
        
        process_module_tree(nil, modules_data)
        
        with_connection do
          ActiveRecord::Base.transaction do
            begin
              # Detect new content items for notification (only if we already had some items, to avoid first-sync flood)
              existing_ids = ContentItem.joins(:content_module).where(content_modules: { course_id: @course_id }).pluck(:brightspace_id).map(&:to_s)
              
              if existing_ids.any? && @all_items_to_upsert.any?
                new_items = @all_items_to_upsert.select { |item| !existing_ids.include?(item[:brightspace_id].to_s) && !item[:is_hidden] }
                if new_items.any?
                  course = Course.find_by(org_unit_id: @course_id)
                  new_items.each do |item|
                    client.upsert_notification({
                      id: "content_#{@course_id}_#{item[:brightspace_id]}",
                      type: 'Content',
                      title: "New Content: #{item[:title]}",
                      body: "A new item has been added to #{course&.name || 'the course'}: #{item[:title]}",
                      date: Time.current,
                      course_id: @course_id,
                      course_name: course&.name,
                      urgency: 1,
                      is_personal: false,
                      url: "/course/#{@course_id}"
                    })
                  end
                end
              end

              ContentModule.upsert_all(@all_modules_to_upsert, unique_by: :index_content_modules_on_course_and_bs_id) if @all_modules_to_upsert.any?
              ContentItem.upsert_all(@all_items_to_upsert, unique_by: :index_content_items_on_module_and_bs_id) if @all_items_to_upsert.any?
            rescue => e
              puts "[Sync::ContentService] Batch upsert failed: #{e.message}"
            end
          end
        end
      end

      private

      def process_module_tree(parent_id, modules_data)
        modules_data.each_with_index do |mod, index|
          m_id = mod['ModuleId'].to_s
          @all_modules_to_upsert << {
            course_id: @course_id,
            brightspace_id: m_id,
            title: mod['Title'],
            description: html_to_markdown(mod['Description']),
            sort_order: index,
            parent_id: parent_id,
            updated_at: Time.current,
            created_at: Time.current
          }
          
          (mod['Topics'] || []).each_with_index do |topic, t_index|
            t_id = (topic['Identifier'] || topic['TopicId'] || topic['Id']).to_s
            @all_items_to_upsert << {
              module_id: m_id,
              brightspace_id: t_id,
              title: topic['Title'],
              item_type: (topic['TypeIdentifier'] || topic['Type']).to_s,
              url: topic['Url'],
              is_hidden: topic['IsHidden'] || false,
              sort_order: t_index,
              attachments: [topic].to_json,
              updated_at: Time.current,
              created_at: Time.current
            }

            if topic['Url'] && (topic['Url'].start_with?('/content/enforced/') || topic['Url'].include?('/viewContent/'))
              client.enqueue_attachment_task(@course_id, "content/topics/#{t_id}/file", t_id, topic['Title'])
            end
          end
          
          process_module_tree(m_id, mod['Modules']) if mod['Modules']
        end
      end
    end
  end
end
