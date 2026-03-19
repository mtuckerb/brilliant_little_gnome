module Brilliant
  module Sync
    class ContentService < BaseService
      def sync(course_id, toc)
        modules_data = toc.is_a?(Hash) ? (toc['Modules'] || []) : toc
        return unless modules_data.is_a?(Array)
        
        @all_modules_to_upsert = []
        @all_items_to_upsert = []
        @course_id = course_id.to_s
        @sync_now = Time.current
        preload_existing_content!
        
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

      def preload_existing_content!
        @existing_modules = ContentModule.where(course_id: @course_id).index_by { |m| m.brightspace_id.to_s }
        @existing_items = ContentItem.joins(:content_module)
                                     .where(content_modules: { course_id: @course_id })
                                     .index_by { |i| "#{i.module_id}|#{i.brightspace_id}" }
      end

      def process_module_tree(parent_id, modules_data)
        modules_data.each_with_index do |mod, index|
          m_id = mod['ModuleId'].to_s
          module_attrs = {
            course_id: @course_id,
            brightspace_id: m_id,
            title: mod['Title'],
            description: html_to_markdown(mod['Description']),
            sort_order: index,
            parent_id: parent_id
          }
          enqueue_module_upsert(module_attrs)
          
          (mod['Topics'] || []).each_with_index do |topic, t_index|
            t_id = (topic['Identifier'] || topic['TopicId'] || topic['Id']).to_s
            item_attrs = {
              module_id: m_id,
              brightspace_id: t_id,
              title: topic['Title'],
              item_type: (topic['TypeIdentifier'] || topic['Type']).to_s,
              url: normalize_content_item_url(topic['Url']),
              is_hidden: topic['IsHidden'] || false,
              sort_order: t_index,
              attachments: [topic].to_json
            }
            enqueue_item_upsert(item_attrs)

            if topic['Url'] && (topic['Url'].start_with?('/content/enforced/') || topic['Url'].include?('/viewContent/'))
              client.enqueue_attachment_task(@course_id, "content/topics/#{t_id}/file", t_id, topic['Title'])
            end
          end
          
          process_module_tree(m_id, mod['Modules']) if mod['Modules']
        end
      end

      def enqueue_module_upsert(attrs)
        existing = @existing_modules[attrs[:brightspace_id].to_s]
        if existing
          unchanged = existing.course_id.to_s == attrs[:course_id].to_s &&
                      existing.title.to_s == attrs[:title].to_s &&
                      existing.description.to_s == attrs[:description].to_s &&
                      existing.sort_order.to_i == attrs[:sort_order].to_i &&
                      existing.parent_id.to_s == attrs[:parent_id].to_s
          return if unchanged
        end

        @all_modules_to_upsert << attrs.merge(
          updated_at: @sync_now,
          created_at: existing ? existing.created_at : @sync_now
        )
      end

      def enqueue_item_upsert(attrs)
        existing = @existing_items["#{attrs[:module_id]}|#{attrs[:brightspace_id]}"]
        if existing
          unchanged = existing.module_id.to_s == attrs[:module_id].to_s &&
                      existing.brightspace_id.to_s == attrs[:brightspace_id].to_s &&
                      existing.title.to_s == attrs[:title].to_s &&
                      existing.item_type.to_s == attrs[:item_type].to_s &&
                      existing.url.to_s == attrs[:url].to_s &&
                      existing.is_hidden == attrs[:is_hidden] &&
                      existing.sort_order.to_i == attrs[:sort_order].to_i &&
                      existing.attachments.to_s == attrs[:attachments].to_s
          return if unchanged
        end

        @all_items_to_upsert << attrs.merge(
          updated_at: @sync_now,
          created_at: existing ? existing.created_at : @sync_now
        )
      end

      def normalize_content_item_url(url)
        return nil if url.nil? || url.to_s.strip.empty?
        value = url.to_s.strip
        return value if value.match?(%r{\Ahttps?://}i)

        host = client.host.to_s.strip
        host = "courses.maine.edu" if host.empty?
        value.start_with?("/") ? "https://#{host}#{value}" : "https://#{host}/#{value}"
      end
    end
  end
end
