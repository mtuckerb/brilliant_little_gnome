module Brilliant
  module Sync
    class ContentService < BaseService
      def sync_content_for_course(course)
        with_connection do
          toc = client.get_toc(course.org_unit_id)
          overview = client.get_overview(course.org_unit_id, force_refresh: true)

          ActiveRecord::Base.transaction do
            refresh_course_overview(course, overview)
            stale_module_ids = course.content_modules.pluck(:id)
            synced_module_ids = []

            toc.fetch("Modules", []).each do |module_data|
              synced_module_ids.concat(sync_module(course, module_data))
            end

            delete_stale_modules(course, stale_module_ids - synced_module_ids)
          end
        end
      end

      def sync_all_content
        Course.all.each do |course|
          sync_content_for_course(course)
        rescue => e
          puts "Failed to sync content for #{course.name}: #{e.message}"
        end
      end

      private

      def refresh_course_overview(course, overview)
        course.update!(overview_raw: overview.to_json) if course.respond_to?(:overview_raw=)
      end

      def delete_stale_modules(course, stale_module_ids)
        return if stale_module_ids.empty?

        ContentModule.where(course_id: course.org_unit_id, id: stale_module_ids).find_each do |stale_module|
          ContentItem.where(module_id: stale_module.brightspace_id).delete_all
          stale_module.destroy!
        end
      end

      def sync_module(course, module_data, parent_id = nil)
        module_record = ContentModule.find_or_initialize_by(
          course_id: course.org_unit_id,
          brightspace_id: module_data["ModuleId"]
        )
        module_record.assign_attributes(
          title: module_data["Title"],
          description: html_to_markdown(module_data["Description"]&.dig("Html")),
          sort_order: module_data["SortOrder"],
          parent_id: parent_id,
          user_id: course.user_id
        )
        module_record.save!

        stale_item_ids = ContentItem.where(module_id: module_record.brightspace_id).pluck(:id)
        synced_item_ids = []
        module_data["Topics"]&.each do |topic_data|
          synced_item_ids << sync_topic(module_record, topic_data)
        end

        stale_item_ids -= synced_item_ids
        ContentItem.where(module_id: module_record.brightspace_id, id: stale_item_ids).delete_all unless stale_item_ids.empty?

        synced_module_ids = [module_record.id]

        # Sync submodules recursively
        module_data["Modules"]&.each do |submodule_data|
          synced_module_ids.concat(sync_module(course, submodule_data, module_record.brightspace_id))
        end

        synced_module_ids
      end

      def sync_topic(module_record, topic_data)
        item = ContentItem.find_or_initialize_by(
          module_id: module_record.brightspace_id,
          brightspace_id: topic_data["TopicId"]
        )

        item.assign_attributes(
          title: topic_data["Title"],
          item_type: topic_data["Type"],
          url: topic_data["Url"],
          is_hidden: topic_data["IsHidden"],
          sort_order: topic_data["SortOrder"],
          attachments: topic_data["Attachments"].to_json,
          user_id: module_record.user_id
        )
        item.save!
        item.id
      end
    end
  end
end
