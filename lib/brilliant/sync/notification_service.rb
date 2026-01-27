module Brilliant
  module Sync
    class NotificationService < BaseService
      def sync(courses, user, full_sync: false, limit: nil)
        # Ensure course cache is primed
        @course_model_cache = Course.all.index_by(&:org_unit_id)
        
        last_sync_key = "last_notification_sync_at"
        last_sync_time = full_sync ? nil : UserPreference.get(last_sync_key)

        # 1. Sync Enrollment Metadata (Unified approach)
        sync_enrollments(courses)

        # 2. Sync Unified Feed
        feed_items = client.get_unified_feed(courses, since: last_sync_time)
        upsert_notification_batch(feed_items, publish_event_flag: true)

        # 3. Course Specific Sync (News, Overviews)
        courses_to_sync = limit ? courses.take(limit) : (full_sync ? courses : courses.take(15))
        sync_course_specific_notifications(courses_to_sync, full_sync, last_sync_time)

        # 4. Global Alerts (Efficiency)
        if (global_alerts = client.get_global_alerts(since: last_sync_time)).any?
          upsert_notification_batch(client.map_alerts_to_notifications(global_alerts, courses), publish_event_flag: true)
        end

        # 5. Assignment Deadlines
        client.sync_upcoming_assignment_notifications(courses)
        
        unless client.degraded_mode
          UserPreference.set(last_sync_key, Time.current.utc.iso8601)
        end

        publish_event(:notifications_synced, { count: Notification.count })
      end

      def upsert_notification_batch(items, publish_event_flag: false)
        items = items.compact
        return if items.empty?
        
        course_ids = items.map { |i| i[:course_id].to_s }.uniq
        course_names = @course_model_cache ? 
                        @course_model_cache.slice(*course_ids).transform_values(&:name) : 
                        Course.where(org_unit_id: course_ids).pluck(:org_unit_id, :name).to_h

        notifications_to_upsert = items.map do |data|
          {
            external_id: data[:id].to_s,
            course_id: data[:course_id].to_s,
            notification_type: data[:type],
            title: data[:title].to_s,
            body: html_to_markdown(data[:body]),
            date: (Time.zone.parse(data[:date].to_s) rescue Time.current),
            course_name: data[:course_name] || course_names[data[:course_id].to_s],
            semester: (data[:semester] || client.extract_semester_from_name(data[:course_name])),
            attachments: data[:attachments],
            urgency: data[:urgency] || 1,
            is_personal: data[:is_personal] || false,
            url: data[:url],
            updated_at: Time.current,
            created_at: Time.current
          }
        end

        begin
          # Filter for truly new notifications to avoid spamming flash notifications
          existing_ids = Notification.where(external_id: items.map { |i| i[:id].to_s }).pluck(:external_id)
          new_items = items.reject { |i| existing_ids.include?(i[:id].to_s) }

          Notification.upsert_all(notifications_to_upsert, unique_by: [:course_id, :external_id])
          
          if publish_event_flag && new_items.size > 0
            # If it's a small number of new items, publish individual events
            # If it's a large number, maybe just one summary event to avoid UI lag
            if new_items.size <= 3
              new_items.each do |n_data|
                publish_event(:notification_received, {
                  id: n_data[:id],
                  title: n_data[:title],
                  body: n_data[:body],
                  course: n_data[:course_name] || course_names[n_data[:course_id].to_s]
                })
              end
            else
              publish_event(:notification_received, {
                id: "batch_#{Time.now.to_i}",
                title: "#{new_items.size} New Notifications",
                body: "Multiple new items have been synced to your feed.",
                course: "System"
              })
            end
          end
        rescue => e
          puts "[Sync::NotificationService] Batch upsert failed: #{e.message}"
        end
      end

      private

      def sync_enrollments(courses)
        ActiveRecord::Base.transaction do
          courses.each do |c|
            next if c.nil? || c['OrgUnit'].nil?
            course = Course.find_or_initialize_by(org_unit_id: c['OrgUnit']['Id'].to_s)
            course.name = c['OrgUnit']['Name'] if c['OrgUnit']['Name'].present? && !c['OrgUnit']['Name'].match?(/^\d+$/)
            course.code = c['OrgUnit']['Code']
            course.is_pinned = !c['PinDate'].nil?
            course.last_accessed_at = (Time.zone.parse(c.dig('Access', 'LastAccessed')) rescue nil)
            course.semester = client.extract_semester_from_name(course.name)
            
            img_url = c.dig('OrgUnit', 'ImageUrl') || c.dig('OrgUnit', 'Image', 'ViewUrl') || c.dig('OrgUnit', 'Image', 'DisplayUrl')
            if img_url && !img_url.empty?
              course.banner_url = img_url.start_with?("/") ? "https://#{client.host}#{img_url}" : img_url
            end
            course.save!
            @course_model_cache[course.org_unit_id] = course
          end
        end
        client.archive_cache("/d2l/api/lp/1.40/enrollments/myenrollments/")
      end

      def sync_course_specific_notifications(courses, full_sync, last_sync_time)
        courses.each do |c|
          course_id = c['OrgUnit']['Id']
          news_path = "/d2l/api/le/1.40/#{course_id}/news/"
          
          cache_rec = ApiCache.active.find_by(path: news_path)
          needs_fresh = !cache_rec || (Time.current - cache_rec.updated_at > 300)
          
          news_data = client.do_get(news_path, force_refresh: (full_sync || needs_fresh))
          items = client.ensure_array(news_data)
          
          news_notifications = items.map do |item|
            body = item.dig('Summary', 'Text') || item.dig('Body', 'Text')
            next if body.to_s.empty?

            {
              id: "news_#{course_id}_#{item['Id']}",
              type: 'News',
              title: item['Title'],
              body: body,
              date: item['StartDate'] || item['LastModifiedDate'] || item['CreatedDate'],
              course_id: course_id,
              course_name: c['OrgUnit']['Name'],
              urgency: 1,
              attachments: item['Attachments']&.to_json,
              is_personal: false,
              url: "/course/#{course_id}/announcements"
            }
          end.compact
          
          upsert_notification_batch(news_notifications, publish_event_flag: true)

          # Overview Sync
          overview = client.get_overview(course_id)
          if overview
            c_record = @course_model_cache[course_id.to_s]
            c_record.update_columns(overview_raw: overview.to_json) if c_record
            
            if overview['Description']&.fetch('Text', nil) || overview['Title']
              upsert_notification_batch([{
                id: "overview_#{course_id}",
                type: 'Content',
                title: "Course Overview: #{overview['Title'] || 'Updated'}",
                body: overview.dig('Description', 'Text') || "The course overview has been updated.",
                date: overview['LastModifiedDate'],
                course_id: course_id,
                course_name: c['OrgUnit']['Name'],
                urgency: 1,
                is_personal: false,
                url: "/course/#{course_id}"
              }], publish_event_flag: true)
            end
          end
        end

        # Content updates for the batch
        content_updates = client.get_content_notifications(courses, since: last_sync_time)
        upsert_notification_batch(content_updates, publish_event_flag: true)
      end
    end
  end
end
