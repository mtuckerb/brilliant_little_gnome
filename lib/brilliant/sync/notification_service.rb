module Brilliant
  module Sync
    class NotificationService < BaseService
      def sync(courses, user, full_sync: false, limit: nil)
        # Ensure course cache is primed
        @course_model_cache = Course.all.index_by(&:org_unit_id)
        
        last_sync_key = "last_notification_sync_at"
        last_sync_time = nil
        begin
          last_sync_time = full_sync ? nil : ::UserPreference.get(last_sync_key)
        rescue => e
          puts "[Sync::NotificationService] Error getting last sync time: #{e.message}"
        end

        # Track all changes detected during this sync session to publish a single aggregate event
        @session_changes = []

        # 1. Sync Enrollment Metadata (Unified approach)
        sync_enrollments(courses)

        # 2. Sync Unified Feed
        feed_items = client.get_unified_feed(courses, since: last_sync_time, force_refresh: full_sync)
        @session_changes += upsert_notification_batch(feed_items, publish_event_flag: false)

        # 3. Course Specific Sync (News, Overviews, Content Updates)
        # Increase coverage to 60 courses to ensure we don't miss notifications from active but non-pinned courses
        courses_to_sync = limit ? courses.take(limit) : (full_sync ? courses : courses.take(60))
        sync_course_specific_notifications(courses_to_sync, full_sync, last_sync_time)

        # 4. Global Alerts (Efficiency)
        if (global_alerts = client.get_global_alerts(since: last_sync_time)).any?
          @session_changes += upsert_notification_batch(client.map_alerts_to_notifications(global_alerts, courses), publish_event_flag: false)
        end

        # 5. Assignment Deadlines
        client.sync_upcoming_assignment_notifications(courses)
        
        unless client.degraded_mode
          ::UserPreference.set(last_sync_key, Time.current.utc.iso8601) rescue nil
        end

        # Now that we've finished the batch, publish a single summary if there were many changes,
        # or individual events if there were only a few.
        publish_aggregated_changes(@session_changes)
        
        # Cleanup session tracking
        @session_changes = nil

        publish_event(:notifications_synced, { count: Notification.count })
      end

      def upsert_notification_batch(items, publish_event_flag: false)
        items = items.compact
        return [] if items.empty?
        
        course_ids = items.map { |i| i[:course_id].to_s }.uniq
        course_names = @course_model_cache ? 
                        @course_model_cache.slice(*course_ids).transform_values(&:name) : 
                        Course.where(org_unit_id: course_ids).pluck(:org_unit_id, :name).to_h

        # Pre-fetch existing notifications to preserve state and detect changes
        requested_external_ids = items.map { |i| i[:id].to_s }
        existing_notifications = Notification.where(external_id: requested_external_ids).index_by(&:external_id)
        
        # Determine current user identity for upsert consistency (upsert_all skips callbacks)
        current_uid = ::UserPreference.current&.brightspace_uid rescue nil

        notifications_to_upsert = items.map do |data|
          external_id = data[:id].to_s
          existing = existing_notifications[external_id]

          # Select the most recent date available to ensure updates move to the top
          dates = [data[:date], data[:last_modified], data[:created_at], data[:start_date]].compact
          parsed_dates = dates.map { |d| (Time.zone.parse(d.to_s) rescue nil) }.compact
          best_date = parsed_dates.max

          # FALLBACK: Use existing date if API returns none. If brand new, use Time.current.
          final_date = best_date || (existing ? existing.date : Time.current)

          {
            external_id: external_id,
            course_id: data[:course_id].to_s,
            notification_type: data[:type],
            title: data[:title].to_s,
            body: html_to_markdown(data[:body]),
            date: final_date,
            course_name: data[:course_name] || course_names[data[:course_id].to_s],
            semester: (data[:semester] || client.extract_semester_from_name(data[:course_name])),
            attachments: data[:attachments],
            urgency: data[:urgency] || 1,
            is_personal: data[:is_personal] || false,
            url: data[:url],
            user_id: current_uid,
            updated_at: Time.current,
            created_at: (existing ? existing.created_at : Time.current),
            is_read: (existing ? existing.is_read : false)
          }
        end

        begin
          changes_detected = []
          
          notifications_to_upsert.each do |n|
            existing = existing_notifications[n[:external_id]]
            if existing.nil?
              changes_detected << n
            else
              # Detect any significant change to trigger a refresh/event
              content_changed = n[:body] != existing.body || n[:title] != existing.title
              
              # Date changes (more than 1 minute difference in either direction)
              date_diff = (n[:date].to_i - existing.date.to_i).abs rescue 0
              date_changed = date_diff > 60
              
              # Force reset unread if content changed significantly
              if content_changed || (date_diff > 3600) # 1 hour jump
                n[:is_read] = false
              end

              if content_changed || date_changed
                changes_detected << n
              end
            end
          end

          Notification.upsert_all(notifications_to_upsert, unique_by: [:course_id, :external_id])
          
          if publish_event_flag && !@session_changes
            # Only publish immediately if we aren't currently in a big sync session
            publish_aggregated_changes(changes_detected)
          end
          
          changes_detected
        rescue => e
          puts "[Sync::NotificationService] Batch upsert failed: #{e.message}"
          puts e.backtrace.first(10).join("\n")
          []
        end
      end

      private

      def publish_aggregated_changes(changes)
        return if changes.empty?
        
        # If it's a small number of changes, publish individual events for nice toast notifications
        if changes.size <= 3
          changes.each do |n_data|
            # We don't have existing_notifications here, but we can infer it's an update if ID exists in system
            # Actually, to keep it simple and clean for the user:
            publish_event(:notification_received, {
              id: n_data[:external_id],
              title: n_data[:title],
              body: n_data[:body],
              course: n_data[:course_name]
            })
          end
        else
          # Aggregate multiple changes into a single summary flash
          publish_event(:notification_received, {
            id: "batch_#{Time.now.to_i}",
            title: "#{changes.size} New/Updated Notifications",
            body: "Multiple items have been updated in your feed.",
            course: "System"
          })
        end
      end

      def sync_enrollments(courses)
        ActiveRecord::Base.transaction do
          courses.each do |c|
            next if c.nil? || c['OrgUnit'].nil?
            course = Course.find_or_initialize_by(org_unit_id: c['OrgUnit']['Id'].to_s)
            incoming_name = c['OrgUnit']['Name']
            course.name = incoming_name if incoming_name.present? && !incoming_name.match?(/^\d+$/)
            unless course.custom_name.present?
              course.code = client.extract_course_code(course.name).presence || c['OrgUnit']['Code']
            end
            course.is_pinned = !c['PinDate'].nil?
            course.last_accessed_at = (Time.zone.parse(c.dig('Access', 'LastAccessed')) rescue nil)
            course.semester = client.extract_semester_from_name(course.display_name)
            
            img_url = extract_banner_url(c['OrgUnit'])
            if img_url && !img_url.empty?
              course.banner_url = normalize_banner_url(img_url)
            end
            course.save!
            @course_model_cache[course.org_unit_id] = course
          end
        end
        client.archive_cache("/d2l/api/lp/1.40/enrollments/myenrollments/")
      end

      BANNER_URL_KEYS = %w[
        ImageUrl
        Image.Url
        Image.ViewUrl
        Image.DisplayUrl
        Image.LargeUrl
        Image.MediumUrl
        Image.SmallUrl
        Image.TileUrl
        Image.ThumbnailUrl
        Image.BannerUrl
        Image.BannerImageUrl
        Image.CardImageUrl
      ].freeze

      def extract_banner_url(org_unit)
        BANNER_URL_KEYS.lazy.map do |key|
          key.split('.').reduce(org_unit) { |value, part| value.respond_to?(:[]) ? value[part] : nil }
        end.find { |value| value.is_a?(String) && !value.empty? }
      end

      def normalize_banner_url(raw_url)
        raw_url.start_with?("/") ? "https://#{client.host}#{raw_url}" : raw_url
      end

      def sync_course_specific_notifications(courses, full_sync, last_sync_time)
        courses.each do |c|
          begin
            course_id = c['OrgUnit']['Id']
            news_path = "/d2l/api/le/1.40/#{course_id}/news/"
            
            cache_rec = ApiCache.active.find_by(path: news_path)
            needs_fresh = !cache_rec || (Time.current - cache_rec.updated_at > 300)
            
            news_data = client.do_get(news_path, force_refresh: (full_sync || needs_fresh))
            items = client.ensure_array(news_data)
            
            news_notifications = items.map do |item|
              # Use the full RichTextInput object (Hash) so that html_to_markdown can handle it
              body_obj = item['Summary'] || item['Body']
              # Only skip if BOTH title and body are missing, which shouldn't happen
              next if item['Title'].to_s.empty? && (body_obj.nil? || body_obj.to_s.empty?)

              {
                id: "news_#{course_id}_#{item['Id']}",
                type: 'News',
                title: item['Title'],
                body: body_obj,
                date: item['LastModifiedDate'] || item['StartDate'] || item['CreatedDate'],
                last_modified: item['LastModifiedDate'],
                start_date: item['StartDate'],
                created_at: item['CreatedDate'],
                course_id: course_id,
                course_name: c['OrgUnit']['Name'],
                urgency: 1,
                attachments: item['Attachments']&.to_json,
                is_personal: false,
                url: "/course/#{course_id}/announcements"
              }
            end.compact
            
            results = upsert_notification_batch(news_notifications, publish_event_flag: false)
            @session_changes += results if @session_changes

            # Content updates for the course
            content_updates = client.get_content_notifications([c], since: last_sync_time)
            results = upsert_notification_batch(content_updates, publish_event_flag: false)
            @session_changes += results if @session_changes

            # Overview Sync
            overview = client.get_overview(course_id)
            if overview
              c_record = @course_model_cache[course_id.to_s]
              c_record.update_columns(overview_raw: overview.to_json) if c_record
              
              if overview['Description']&.fetch('Text', nil) || overview['Title']
                results = upsert_notification_batch([{
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
                }], publish_event_flag: false)
                @session_changes += results if @session_changes
              end
            end
          rescue => e
            puts "[Sync::NotificationService] Error syncing course #{c.dig('OrgUnit', 'Id')}: #{e.message}"
            # Continue to next course
          end
        end
      end
    end
  end
end
