require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'
require 'time'

class BrilliantClient
  class AuthenticationError < StandardError; attr_reader :status_code; def initialize(msg, code); super(msg); @status_code = code; end; end
  
  attr_accessor :token, :cookie_string, :host, :user_display_name, :sync_status, :degraded_mode, :last_auth_error

  def initialize
    data_dir = ENV['BRILLIANT_DATA_DIR'] || '.'
    @config_path = File.join(data_dir, 'config', 'connection.json')
    load_connection_config
    
    @api_version = "1.40"
    @sync_lock = Mutex.new
    @syncing = false
    @sync_status = { status: "idle", progress: 0, current_task: nil }
    @degraded_mode = false
    @last_auth_error = nil
    @auth_notification_sent = false
    @in_flight_requests = {} # Track active GET requests: { path => Mutex }
    @pending_tasks = {}      # Track paths in @task_queue to avoid duplicates

    # Background Worker for low-priority tasks (attachments, etc.)
    @task_queue = Queue.new
    @worker_thread = Thread.new do
      loop do
        task_data = @task_queue.pop
        begin
          ActiveRecord::Base.connection_pool.with_connection do
            if task_data.is_a?(Hash) && task_data[:path]
              fetch_and_cache(task_data[:path])
              @pending_tasks.delete(task_data[:path])
            elsif task_data.respond_to?(:call)
              task_data.call
            end
          end
        rescue => e
          puts "[Worker] Task failed: #{e.message}"
        end
      end
    end

    # Periodically clean up old cache entries
    Thread.new { sleep 60; ActiveRecord::Base.connection_pool.with_connection { cleanup_cache } rescue nil }
    
    # Legacy/Fallback: Try loading from cookies.txt
    cookies_fallback = File.join(data_dir, 'cookies.txt')
    load_cookies_from_file(cookies_fallback) if !authenticated? && File.exist?(cookies_fallback)
  end

  def load_connection_config
    if File.exist?(@config_path)
      config = JSON.parse(File.read(@config_path))
      @host = config['host'] || ENV['BS_HOST'] || "courses.maine.edu"
      @cookie_string = config['cookies']&.sub(/^Cookie:\s*/i, '')
    else
      @host = ENV['BS_HOST'] || "courses.maine.edu"
    end
    
    @client_id = ENV['BS_CLIENT_ID']
    @client_secret = ENV['BS_CLIENT_SECRET']
    @redirect_uri = ENV['BS_REDIRECT_URI'] || "http://localhost:4567/callback"
  end

  def save_connection_config(host, cookies)
    @host = host.to_s.gsub(/https?:\/\//, '').split('/').first
    @cookie_string = cookies.sub(/^Cookie:\s*/i, '')
    @degraded_mode = false # Reset degraded mode on new config
    
    FileUtils.mkdir_p(File.dirname(@config_path))
    File.write(@config_path, { host: @host, cookies: @cookie_string }.to_json)
  end

  def sync_all_courses_proactively
    return if @syncing
    
    Thread.new do
      ActiveRecord::Base.connection_pool.with_connection do
        # Ensure timezone is set for the background thread
        Time.zone = UserPreference.current&.time_zone || "UTC"
        
        @sync_lock.synchronize { @syncing = true }
        @sync_status = { status: "syncing", progress: 0, current_task: "Starting proactive sync..." }
        
        begin
          # Check for a "full sync" request
          full_sync = UserPreference.get('force_full_sync') == 'true'
          skipped_items = []
          
          courses = get_enrollments(force_refresh: full_sync) || []
          puts "[Sync] Number of courses to process: #{courses.size}"
          user = get_who_am_i
          
          unless user && user['Identifier']
            raise "Could not retrieve user identity. Please relogin."
          end

          total_steps = 1 + courses.size # Notifications + each course
          current_step = 0

          @sync_status[:current_task] = "Syncing Notifications..."
          sync_notifications(courses, user, full_sync: full_sync)

          # Reset full sync flag if set
          UserPreference.set('force_full_sync', 'false') if full_sync

          current_step += 1
          @sync_status[:progress] = ((current_step.to_f / total_steps) * 100).to_i

          courses.each do |c|
            course_id = c['OrgUnit']['Id']
            course_name = c['OrgUnit']['Name']
            puts "[Sync] Processing Course: #{course_name} (ID: #{course_id})"
            
            # Simple truncation helper for status display
            short_name = course_name.length > 10 ? course_name[0...9] + "…" : course_name

            @sync_status[:current_task] = "#{short_name} - Syncing Core Content..."
            
            # Sync Core
            toc_path = "/d2l/api/le/#{@api_version}/#{course_id}/content/toc"
            toc = get_toc(course_id)
            puts "[Sync] TOC for #{course_id} fetched: #{toc.is_a?(Hash) ? toc.keys : toc.class}"
            sync_course_content(course_id, toc) if toc
            archive_cache(toc_path) if toc
            
            sleep 0.1
            @sync_status[:current_task] = "#{short_name} - Syncing Assignments..."
            assign_path = "/d2l/api/le/#{@api_version}/#{course_id}/dropbox/folders/"
            assignments = get_assignments(course_id)
            puts "[Sync] Assignments for #{course_id} fetched: #{assignments.is_a?(Array) ? assignments.size : assignments.class}"
            skipped_items += sync_assignments(course_id, assignments) if assignments
            archive_cache(assign_path) if assignments

            sleep 0.1
            @sync_status[:current_task] = "#{short_name} - Syncing Quizzes..."
            quiz_path = "/d2l/api/le/#{@api_version}/#{course_id}/quizzes/"
            quizzes = get_quizzes(course_id)
            skipped_items += sync_quizzes(course_id, quizzes) if quizzes
            archive_cache(quiz_path) if quizzes
            
            sleep 0.1
            
            @sync_status[:current_task] = "#{short_name} - Syncing Discussions..."
            # Sync Discussions
            disc_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/"
            forums = get_discussions(course_id) || []
            sync_discussions(course_id, forums) if forums.any?
            archive_cache(disc_path) if forums.any?
            
            sleep 0.1
            
            @sync_status[:current_task] = "#{short_name} - Syncing Grades..."
            # Sync Grades
            grades_path = "/d2l/api/le/#{@api_version}/#{course_id}/grades/values/myGradeValues/"
            grades_raw = get_grades(course_id)
            sync_grades(course_id, grades_raw) if grades_raw.is_a?(Array)
            archive_cache(grades_path) if grades_raw.is_a?(Array)

            current_step += 1
            @sync_status[:progress] = ((current_step.to_f / total_steps) * 100).to_i

            # Publish course-wide update event
            Brilliant::EventBus.publish(:course_overview_updated, { course_id: course_id })
            Brilliant::EventBus.publish(:assignments_updated, { course_id: course_id })
            Brilliant::EventBus.publish(:grades_updated, { course_id: course_id })
          end
          
          # Notify user about skipped items (manual overrides protected)
          if skipped_items.any?
            create_system_notification({
              id: "sync_protection_#{Time.current.to_i}",
              title: "Sync Protection: Manual Edits Preserved",
              body: "#{skipped_items.size} assignment(s) were not updated because they contain your manual edits: #{skipped_items.take(3).map{|i| i.name}.join(', ')}#{'...' if skipped_items.size > 3}",
              urgency: 2
            })
          end
          
          @sync_status = { status: "completed", progress: 100, current_task: "Proactive sync complete." }
        rescue => e
          @sync_status = { status: "error", progress: 0, current_task: "Sync error: #{e.message}" }
          shared_puts "Sync error: #{e.message}"
        ensure
          @sync_lock.synchronize { @syncing = false }
        end
      end
    end
  end

  def shared_puts(msg)
    puts msg
  end

  def sync_notifications(courses, user, full_sync: false, limit: nil)
    if !full_sync && @last_notif_sync && Time.current - @last_notif_sync < 60
      puts "[Brilliant API] Skipping notification sync (run within last 60s)"
      return
    end

    @last_notif_sync = Time.current
    puts "[Brilliant API] Syncing notifications to DB..."
    
    last_sync_key = "last_notification_sync_at"
    last_sync_time = full_sync ? nil : UserPreference.get(last_sync_key)
    
    # Sync courses to normalized table
    ActiveRecord::Base.transaction do
      @course_model_cache = {}
      courses.each do |c|
        next if c.nil? || c['OrgUnit'].nil?

        new_name = c['OrgUnit']['Name']
        next if new_name.nil? || new_name.empty?

        course = Course.find_or_initialize_by(org_unit_id: c['OrgUnit']['Id'].to_s)
        course.name = new_name if new_name.present? && !new_name.match?(/^\d+$/)
        course.code = c['OrgUnit']['Code']
        course.is_pinned = !c['PinDate'].nil?
        course.last_accessed_at = (Time.zone.parse(c.dig('Access', 'LastAccessed')) rescue nil)
        course.semester = extract_semester_from_name(course.name)
        
        img_url = c.dig('OrgUnit', 'ImageUrl') || c.dig('OrgUnit', 'Image', 'ViewUrl') || c.dig('OrgUnit', 'Image', 'DisplayUrl')
        if img_url && !img_url.empty?
          img_url = "https://#{@host}#{img_url}" if img_url.start_with?("/")
          course.banner_url = img_url
        end
        
        course.save!
        @course_model_cache[course.org_unit_id] = course
      end
    end
    archive_cache("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/")

    # Pre-warm course cache for further lookups if needed
    @course_model_cache ||= Course.all.index_by(&:org_unit_id)

    feed_items = get_unified_feed(courses, since: last_sync_time)
    upsert_notification_batch(feed_items)

    courses_to_sync = limit ? courses.take(limit) : (full_sync ? courses : courses.take(15))
    
    courses_to_sync.each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      next if course_id.nil?
      
      # Check if we've refreshed news recently (within 5 minutes)
      news_path = "/d2l/api/le/#{@api_version}/#{course_id}/news/"
      cache_rec = ApiCache.active.find_by(path: news_path)
      needs_fresh = !cache_rec || (Time.current - cache_rec.updated_at > 300)
      
      news_data = do_get(news_path, force_refresh: (full_sync || needs_fresh))
      items = ensure_array(news_data)
      
      news_notifications = items.map do |item|
        new_body = item.dig('Summary', 'Text') || item.dig('Body', 'Text')
        next if new_body.to_s.empty?

        {
          id: "news_#{course_id}_#{item['Id']}",
          type: 'News',
          title: item['Title'],
          body: new_body,
          date: item['StartDate'] || item['LastModifiedDate'] || item['CreatedDate'],
          course_id: course_id,
          course_name: c['OrgUnit']['Name'],
          urgency: 1,
          attachments: item['Attachments']&.to_json,
          is_personal: false,
          url: "/course/#{course_id}/announcements"
        }
      end.compact
      
      upsert_notification_batch(news_notifications)

      items.take(3).each do |item| # Limit attachment processing in main sync
        if item['Attachments'] && !item['Attachments'].empty?
          item['Attachments'].each do |att|
            @task_queue << proc { persist_attachment(course_id, "news/#{item['Id']}/attachments/#{att['FileId']}", att['FileId'], att['FileName']) }
          end
        end
      end

      overview = get_overview(course_id)
      if overview
        c_record = @course_model_cache[course_id.to_s]
        c_record.update_columns(overview_raw: overview.to_json) if c_record
      end

      if overview && (overview['Description']&.fetch('Text', nil) || overview['Title'])
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
        }])
      end
    end

    # Content notifications and other global syncs
    upsert_notification_batch(get_content_notifications(courses_to_sync, since: last_sync_time))
    
    # Global Delta Sync (Alerts) - Replaces the heavy course-by-course scan
    if (global_alerts = get_global_alerts(since: last_sync_time)).any?
      upsert_notification_batch(map_alerts_to_notifications(global_alerts, courses))
    end
    
    upsert_notification_batch(get_discussion_notifications(courses_to_sync, user['Identifier'])) if user && user['Identifier']

    sync_upcoming_assignment_notifications(courses)
    
    if !@degraded_mode
      UserPreference.set(last_sync_key, Time.current.utc.iso8601)
    end

    # Final single event to refresh UI
    Brilliant::EventBus.publish(:notifications_synced, { count: Notification.count })
    puts "[Brilliant API] Notification sync complete."
  end

  def upsert_notification(data)
    upsert_notification_batch([data], publish_event: true)
  end

  def upsert_notification_batch(items, publish_event: false)
    items = items.compact
    return if items.empty?
    
    # Pre-fetch course display names to avoid repeated lookups
    course_ids = items.map { |i| i[:course_id].to_s }.uniq
    course_names = @course_model_cache ? @course_model_cache.slice(*course_ids).transform_values(&:name) : Course.where(org_unit_id: course_ids).pluck(:org_unit_id, :name).to_h

    notifications_to_upsert = items.map do |data|
      # Sanitize/format fields for DB
      {
        external_id: data[:id].to_s,
        course_id: data[:course_id].to_s,
        notification_type: data[:type],
        title: data[:title].to_s,
        body: html_to_markdown(data[:body]),
        date: (Time.zone.parse(data[:date].to_s) rescue Time.current),
        course_name: data[:course_name] || course_names[data[:course_id].to_s],
        semester: (data[:semester] || extract_semester_from_name(data[:course_name])),
        attachments: data[:attachments],
        urgency: data[:urgency] || 1,
        is_personal: data[:is_personal] || false,
        url: data[:url],
        updated_at: Time.current,
        created_at: Time.current
      }
    end

    begin
      Notification.upsert_all(notifications_to_upsert, unique_by: [:course_id, :external_id])
      
      if publish_event && items.size == 1
        n_data = items.first
        Brilliant::EventBus.publish(:notification_received, {
          id: n_data[:id],
          title: n_data[:title],
          body: n_data[:body],
          course: n_data[:course_name] || course_names[n_data[:course_id].to_s]
        })
      end
    rescue => e
      puts "[Sync] Notification batch upsert failed: #{e.message}"
    end
  end

  def sync_upcoming_assignment_notifications(courses)
    upcoming_limit = Time.current + 7.days
    
    ActiveRecord::Base.connection_pool.with_connection do
      Assignment.where("due_date > ? AND due_date <= ?", Time.current, upcoming_limit).each do |a|
        next if a.nil?
        begin
          course_obj = @course_model_cache ? @course_model_cache[a.course_id.to_s] : Course.find_by(org_unit_id: a.course_id.to_s)
          course_name = course_obj&.name || "Unknown Course"

          type_label = a.assignment_type == 'quiz' ? 'Quiz' : 'Assignment'
          url = a.assignment_type == 'quiz' ? 
                "/course/#{a.course_id}/quizzes/#{a.brightspace_id.sub('quiz_', '')}" : 
                "/course/#{a.course_id}/assignments/#{a.brightspace_id}"
          url += '?edit=true' if a.synthetic
          
          body_text = "This #{type_label.downcase} is due on #{a.due_date.strftime('%A, %b %d at %I:%M %p')}."
          
          if a.synthetic && a.external_url.present?
            body_text += " [Open Link](#{a.external_url})"
          end
          
          upsert_notification({
            id: "upcoming_assignment_#{a.brightspace_id}",
            type: 'Assignment',
            title: "Upcoming #{type_label}: #{a.name}",
            body: body_text,
            date: a.due_date - 1.day,
            course_id: a.course_id,
            course_name: course_name,
            urgency: 2,
            is_personal: true,
            url: url
          })
        rescue => e
          puts "[Sync] Upcoming assignment sync failed for #{a.id}: #{e.message}"
        end
      end
    end
  end

  def get_content_notifications(courses, since: nil)
    all_updates = []
    courses.take(20).each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      
      path = "/d2l/api/le/#{@api_version}/#{course_id}/content/updates"
      path += "?since=#{URI.encode_www_form_component(since)}" if since
      
      data = do_get(path)
      next unless data

      items = ensure_array(data)
      items.each do |item|
        all_updates << {
          id: "content_#{course_id}_#{item['Identifier'] || item['Id']}",
          type: 'Content',
          title: "Content Updated: #{item['Title']}",
          body: "New or updated content in #{c['OrgUnit']['Name']}: #{item['Title']}",
          date: item['CreatedDate'] || item['LastModifiedDate'] || Time.current.iso8601,
          course_id: course_id,
          course_name: c['OrgUnit']['Name'],
          urgency: 1,
          is_personal: false,
          url: "/course/#{course_id}"
        }
      end
      archive_cache(path)
    end
    all_updates
  end

  def get_global_alerts(since: nil)
    path = "/d2l/api/lp/#{@api_version}/alerts/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    
    # Use force_refresh: true to ensure we always try to get the very latest alerts
    # during the notification sync.
    data = do_get(path, force_refresh: true)
    ensure_array(data)
  end

  def map_alerts_to_notifications(alerts, courses)
    course_map = courses.each_with_object({}) { |c, h| h[c['OrgUnit']['Id'].to_s] = c['OrgUnit']['Name'] }
    
    alerts.map do |alert|
      course_id = alert['OrgUnitId'].to_s
      type = alert['Type']
      
      # Brightspace Alerts types: Grade, News, Content, etc.
      {
        id: "alert_#{course_id}_#{alert['Id']}",
        type: type,
        title: alert['Title'] || "#{type} Update",
        body: alert['Text'] || "You have a new #{type.downcase} update in #{course_map[course_id]}.",
        date: alert['Date'] || Time.current.iso8601,
        course_id: course_id,
        course_name: course_map[course_id] || "Course #{course_id}",
        urgency: (type == 'Grade' ? 3 : 1),
        is_personal: (type == 'Grade'),
        url: "/course/#{course_id}" + (type == 'Grade' ? "/grades" : "")
      }
    end
  end

  def get_recent_grades_notifications(courses)
    # This is now a legacy/fallback method, as global alerts handle this 
    # in a much more efficient way. Returning empty to avoid duplication.
    []
  end

  def get_discussion_notifications(courses, user_id)
    # Implement discussion evaluation/reply checks here in the future
    # For now, return empty to avoid the crash encountered earlier
    []
  end

  def extract_semester_from_name(full_name)
    return nil unless full_name
    patterns = [
      /(\d{4}\s+(?:Spring|Fall|Summer|Winter|Session|Quarter))/i,
      /((?:Spring|Fall|Summer|Winter|Session|Quarter)\s+\d{4})/i
    ]
    patterns.each do |p|
      match = full_name.match(p)
      return match[1].strip if match
    end
    nil
  end

  def load_cookies_from_file(path = nil)
    path ||= 'cookies.txt'
    return unless File.exist?(path)
    content = File.read(path).strip
    return if content.empty?

    if content.start_with?('ey')
      @token = content
    else
      @cookie_string = content.sub(/^Cookie:\s*/i, '')
    end
  end

  def auth_url
    "https://#{@host}/oauth2/auth?" + URI.encode_www_form({
      response_type: 'code',
      client_id: @client_id,
      redirect_uri: @redirect_uri,
      scope: 'core:*:* enrollments:*:* content:*:* grades:*:*'
    })
  end

  def exchange_code(code)
    uri = URI("https://#{@host}/core/connect/token")
    request = Net::HTTP::Post.new(uri)
    request.content_type = 'application/x-www-form-urlencoded'
    
    auth_str = Base64.strict_encode64("#{@client_id}:#{@client_secret}")
    request['Authorization'] = "Basic #{auth_str}"
    
    request.set_form_data({
      'grant_type' => 'authorization_code',
      'redirect_uri' => @redirect_uri,
      'code' => code
    })

    response = Net::HTTP.start(uri.hostname, uri.port, use_ssl: true) do |http|
      http.request(request)
    end

    if response.code == '200'
      data = JSON.parse(response.body)
      @token = data['access_token']
      return true
    end
    false
  end

  def authenticated?
    !@token.nil? || !@cookie_string.nil?
  end

  def get_org_unit(org_unit_id)
    do_get("/d2l/api/lp/#{@api_version}/orgstructure/#{org_unit_id}")
  end

  def get_who_am_i
    return @cached_who_am_i if @cached_who_am_i && @last_who_am_i_at && @last_who_am_i_at > Time.now - 300
    
    data = fetch_and_cache("/d2l/api/lp/#{@api_version}/users/whoami")
    if data && data['Identifier']
      @cached_who_am_i = data
      @last_who_am_i_at = Time.now
      @user_display_name = data['DisplayName']
      # Store identity in UserPreference
      ActiveRecord::Base.connection_pool.with_connection do
        pref = UserPreference.current
        pref.update(
          brightspace_uid: data['UniqueIdentifier'], 
          brightspace_user_id: data['Identifier'], 
          last_login_at: Time.current
        )
      end
    end
    data
  end

  def get_enrollments(force_refresh: false)
    response = do_get("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/", force_refresh: force_refresh)
    return [] unless response
    
    items = ensure_array(response)
    items.select do |i| 
      (i.dig('OrgUnit', 'Type', 'Code') == 'Course Offering' || 
       i.dig('OrgUnit', 'Type', 'Name') == 'Course Offering')
    end.sort_by do |i|
      pin_score = i['PinDate'] ? 0 : 1
      raw_access = i.dig('Access', 'LastAccessed')
      begin
        access_time = raw_access ? Time.zone.parse(raw_access) : Time.at(0)
      rescue
        access_time = Time.at(0)
      end
      [pin_score, -access_time.to_i]
    end
  end

  def get_toc(org_unit_id)
    data = do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/content/toc")
    if data.is_a?(Array)
      { 'Modules' => data }
    elsif data.is_a?(Hash) && data['Modules']
      data
    else
      { 'Modules' => [] }
    end
  end

  def get_assignments(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/")
  end

  def get_quizzes(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/quizzes/")
  end

  def get_assignment(org_unit_id, assignment_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/#{assignment_id}")
  end

  def get_assignment_feedback(org_unit_id, assignment_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/#{assignment_id}/feedback/myFeedback")
  end

  def get_assignment_submissions(org_unit_id, assignment_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/#{assignment_id}/submissions/")
  end

  def get_assignment_rubrics(org_unit_id, assignment_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/feedback/rubrics/")
  end

  def get_grades(org_unit_id, force_refresh: false)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/grades/values/myGradeValues/", force_refresh: force_refresh)
  end

  def get_discussions(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/")
  end

  def get_discussion_forum(org_unit_id, forum_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}")
  end

  def get_all_topics(org_unit_id)
    forums = get_discussions(org_unit_id) || []
    all_topics = []
    forums.each do |f|
      topics_data = get_discussion_topics(org_unit_id, f['ForumId'])
      topics = ensure_array(topics_data)
      topics.each { |t| t['ForumId'] = f['ForumId']; t['ForumName'] = f['Name'] }
      all_topics.concat(topics)
    end
    all_topics
  end

  def get_discussion_topics(org_unit_id, forum_id, force_refresh: false)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/", force_refresh: force_refresh)
  end

  def get_discussion_topic(org_unit_id, forum_id, topic_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}")
  end

  def get_discussion_threads(org_unit_id, forum_id, topic_id, force_refresh: false)
    path = "/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/"
    do_get(path, force_refresh: force_refresh)
  end

  def get_discussion_evaluation(org_unit_id, forum_id, topic_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/evaluations/myEvaluation")
  end

  def get_discussion_thread(org_unit_id, forum_id, topic_id, thread_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/#{thread_id}")
  end

  def get_thread_posts(org_unit_id, forum_id, topic_id, thread_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/#{thread_id}/posts/")
  end

  def get_topic_posts(org_unit_id, forum_id, topic_id, force_refresh: false)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/posts/", force_refresh: force_refresh)
  end

  def get_news(org_unit_id, since: nil)
    path = "/d2l/api/le/#{@api_version}/#{org_unit_id}/news/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    do_get(path)
  end

  def get_unified_feed(courses = [], since: nil)
    course_map = courses.each_with_object({}) { |c, h| h[c['OrgUnit']['Id'].to_s] = c['OrgUnit']['Name'] }
    path = "/d2l/api/lp/#{@api_version}/feed/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    
    feed_data = do_get(path)
    feed = ensure_array(feed_data)
    
    feed.map do |item|
      cid = item.dig('Metadata', 'ApiViewUrl')&.match(/\/(\d+)\/news/)&.to_a&.at(1)
      {
        id: item.dig('Metadata', 'Identifier'),
        type: 'News',
        title: item.dig('Metadata', 'Title'),
        body: item.dig('Metadata', 'Summary', 'Text'),
        date: item.dig('Metadata', 'Date') || item.dig('Resource', 'CreatedDate'),
        course_id: cid,
        course_name: cid ? course_map[cid.to_s] : "General",
        urgency: 1,
        is_personal: false,
        url: item.dig('Metadata', 'WebViewUrl') ? "https://#{@host}#{item.dig('Metadata', 'WebViewUrl')}" : nil
      }
    end
  end

  def sync_course_content(course_id, toc)
    modules_data = toc.is_a?(Hash) ? (toc['Modules'] || []) : toc
    return unless modules_data.is_a?(Array)
    
    # Collect all modules and items into flat lists for batch upsert
    @all_modules_to_upsert = []
    @all_items_to_upsert = []
    
    process_module_tree(course_id, nil, modules_data)
    
    ActiveRecord::Base.transaction do
      begin
        ContentModule.upsert_all(@all_modules_to_upsert, unique_by: :index_content_modules_on_course_and_bs_id) if @all_modules_to_upsert.any?
        ContentItem.upsert_all(@all_items_to_upsert, unique_by: :index_content_items_on_module_and_bs_id) if @all_items_to_upsert.any?
      rescue => e
        puts "[Sync] Course content batch upsert failed: #{e.message}"
      end
    end
  end

  def process_module_tree(course_id, parent_id, modules_data)
    modules_data.each_with_index do |mod, index|
      m_id = mod['ModuleId'].to_s
      @all_modules_to_upsert << {
        course_id: course_id.to_s,
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
          @task_queue << proc { persist_attachment(course_id, "content/topics/#{t_id}/file", t_id, topic['Title']) }
        end
      end
      
      process_module_tree(course_id, m_id, mod['Modules']) if mod['Modules']
    end
  end

  def sync_assignments(course_id, assignments)
    items = ensure_array(assignments)
    skipped = []
    
    # 1. Fetch detailed information for all assignments in parallel/batches if possible
    # but for now, we'll just process the summaries we have and enrich them
    # Note: the summary often lacks the full description, but for performance,
    # we'll sync what we have and only fetch details on-demand or in background.
    
    assignments_to_upsert = items.map do |a_summary|
      next if a_summary.nil?
      t_id = (a_summary['Id'] || a_summary['Identifier'] || a_summary['TopicId']).to_s
      
      # For now, we use the summary data. Detailed enrichment could be backgrounded.
      # If we already have the record and it's manually edited, skip it.
      existing = Assignment.find_by(brightspace_id: t_id, course_id: course_id.to_s)
      if existing&.manually_edited?
        skipped << existing
        next
      end

      # Handle instruction/description objects which often contain HTML
      desc_obj = a_summary['CustomInstructions'] || a_summary['Description']
      desc_text = html_to_markdown(desc_obj)

      atts = (a_summary['Attachments'] || []) + (a_summary['LinkAttachments'] || [])
      
      {
        course_id: course_id.to_s,
        brightspace_id: t_id,
        name: a_summary['Name'],
        due_date: (Time.zone.parse(a_summary['DueDate']) rescue nil),
        description: desc_text,
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
      puts "[Sync] Assignment batch upsert failed: #{e.message}"
      # Fallback to individual saves if necessary
    end

    skipped
  end

  def sync_quizzes(course_id, quizzes)
    items = ensure_array(quizzes)
    skipped = []
    
    quizzes_to_upsert = items.map do |q|
      next if q.nil?
      q_id = (q['QuizId'] || q['Id'] || q['Identifier']).to_s
      
      existing = Assignment.find_by(brightspace_id: "quiz_#{q_id}", course_id: course_id.to_s)
      if existing&.manually_edited?
        skipped << existing
        next
      end

      desc_obj = q['Description'] || q['Header']
      desc_text = html_to_markdown(desc_obj)

      # Add instructions if present
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
      puts "[Sync] Quiz batch upsert failed: #{e.message}"
    end

    skipped
  end

  def html_to_markdown(html)
    return "" if html.nil?
    
    # Handle D2L Rich Text Hash objects
    if html.is_a?(Hash)
      html = html["Html"] || html["Text"] || html[:Html] || html[:Text] || ""
    end
    
    # Handle stringified Ruby hashes (recovery from sync bugs)
    if html.is_a?(String) && html.start_with?("{") && html.include?("=>")
      if html =~ /"(?:Html|Text)"\s*=>\s*"(.*?)"/m
        html = $1
      elsif html =~ /:(?:Html|Text)\s*=>\s*"(.*?)"/m
        html = $1
      end
    end

    return "" if html.to_s.empty?
    
    # Very basic HTML to Markdown conversion
    text = html.to_s.dup
    text.gsub!(/<br\s*\/?>/i, "\n")
    text.gsub!(/<\/p>/i, "\n\n")
    text.gsub!(/<p[^>]*>/i, "")
    text.gsub!(/<strong>(.*?)<\/strong>/i, '**\1**')
    text.gsub!(/<b>(.*?)<\/b>/i, '**\1**')
    text.gsub!(/<em>(.*?)<\/em>/i, '_\1_')
    text.gsub!(/<i>(.*?)<\/i>/i, '_\1_')
    text.gsub!(/<a\s+[^>]*href="([^"]*)"[^>]*>(.*?)<\/a>/i, '[\2](\1)')
    text.gsub!(/<ul[^>]*>/i, "\n")
    text.gsub!(/<\/ul>/i, "\n")
    text.gsub!(/<li[^>]*>/i, "\n* ")
    text.gsub!(/<\/li>/i, "")
    
    # Strip remaining tags
    text.gsub!(/<[^>]+>/, '')
    
    # Decode common entities
    text.gsub!("&nbsp;", " ")
    text.gsub!("&amp;", "&")
    text.gsub!("&lt;", "<")
    text.gsub!("&gt;", ">")
    text.gsub!("&quot;", "\"")

    text.strip
  end

  def sync_discussions(course_id, forums)
    # 1. Sync Forums
    ActiveRecord::Base.transaction do
      ensure_array(forums).each do |f|
        forum = DiscussionForum.find_or_initialize_by(brightspace_id: f['ForumId'].to_s, course_id: course_id.to_s)
        forum.name = f['Name']
        forum.description = html_to_markdown(f['Description'])
        forum.save!
      end
    end

    # 2. Sync Topics for the whole course at once (much faster than per-forum)
    # Brightspace API usually supports /d2l/api/le/(version)/(orgUnitId)/discussions/topics/
    all_topics_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/topics/"
    topics_raw = do_get(all_topics_path)
    if topics_raw
      topics = ensure_array(topics_raw)
      sync_discussion_topics(course_id, nil, topics)
    else
      # Fallback to per-forum if course-level list fails
      ensure_array(forums).each do |f|
        topics = ensure_array(get_discussion_topics(course_id, f['ForumId']))
        sync_discussion_topics(course_id, f['ForumId'], topics)
      end
    end
  end

  def sync_discussion_topics(course_id, forum_id, topics)
    ActiveRecord::Base.transaction do
      topics.each_with_index do |t, index|
        # Use provided forum_id OR extract from topic data if course-level sync
        fid = forum_id || t['ForumId']
        next unless fid

        topic = DiscussionTopic.find_or_initialize_by(brightspace_id: t['TopicId'].to_s, forum_id: fid.to_s)
        topic.course_id = course_id.to_s
        topic.name = t['Name']
        topic.description = html_to_markdown(t['Description'])
        topic.sort_order = index
        # We can also capture counts if they are in the metadata
        topic.thread_count = t['ThreadCount'] if t['ThreadCount']
        topic.post_count = t['PostCount'] if t['PostCount']
        topic.last_post_date = Time.zone.parse(t['LastPostDate']) rescue nil if t['LastPostDate']
        
        topic.save!
      end
    end
  end

  def sync_grades(course_id, grade_values)
    items = ensure_array(grade_values)
    return if items.empty?

    grades_to_upsert = items.map do |g|
      obj_id = (g['GradeObjectIdentifier'] || g['Identifier']).to_s
      {
        course_id: course_id.to_s,
        brightspace_id: obj_id,
        name: g['GradeObjectName'] || "Grade Item #{obj_id}",
        displayed_grade: g['DisplayedGrade'],
        numerator: (g.dig('GradeValue', 'Numerator') || g['Numerator']), # Capture numeric values if present
        denominator: (g.dig('GradeValue', 'Denominator') || g['Denominator']),
        updated_at: Time.current,
        created_at: Time.current
      }
    end

    begin
      # Use course_id and brightspace_id as unique key (indexed earlier)
      Grade.upsert_all(grades_to_upsert, unique_by: [:course_id, :brightspace_id])
    rescue => e
      puts "[Sync] Grade batch upsert failed: #{e.message}"
    end
  end

  def get_recent_grades_notifications(courses)
    alerts = []
    courses.each do |c|
      course_id = c['OrgUnit']['Id']
      ensure_array(get_grades(course_id)).each do |g|
        next unless g['DisplayedGrade']
        alerts << {
          id: "grade_#{course_id}_#{g['GradeObjectIdentifier']}",
          type: 'Grade',
          title: "Grade Updated: #{g['GradeObjectName']}",
          body: "Your grade for #{g['GradeObjectName']} is now #{g['DisplayedGrade']}.",
          course_id: course_id,
          course_name: c['OrgUnit']['Name'],
          urgency: 3,
          is_personal: true,
          url: "/course/#{course_id}/grades"
        }
      end
    end
    alerts
  end

  def get_discussion_notifications(courses, user_id)
    # Placeholder for discussion notification logic
    []
  end

  def get_overview(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/overview")
  end

  def download_file(path, limit = 5)
    return nil unless authenticated?
    uri = path.start_with?('http') ? URI.parse(path) : URI("https://#{@host}/#{path.sub(/^\//, '')}")
    request = Net::HTTP::Get.new(uri)
    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string.sub(/^Cookie:\s*/i, '')
    end

    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true
    
    begin
      response = http.request(request)
      if response.code =~ /^30/ && limit > 0
        return download_file(response['location'], limit - 1)
      end
      response
    rescue
      nil
    end
  end

  def persist_attachment(course_id, api_path, file_id, file_name)
    path = api_path.start_with?('/') ? api_path : "/d2l/api/le/#{@api_version}/#{course_id}/#{api_path}"
    data_dir = ENV['BRILLIANT_DATA_DIR'] || '.'
    local_dir = File.join(data_dir, 'public', 'attachments', course_id.to_s)
    FileUtils.mkdir_p(local_dir)
    
    local_path = File.join(local_dir, "#{file_id}_#{file_name.gsub(/[^0-9A-Za-z.\- ]/, '_')}")
    return local_path if File.exist?(local_path)

    response = download_file(path)
    if response && response.code == '200'
      File.open(local_path, 'wb') { |f| f.write(response.body) }
      return local_path
    end
    nil
  end

  def portal_url_for(type, options = {})
    course_id = options[:course_id]
    id = options[:id]
    base_url = "https://#{@host}/d2l"
    case type
    when :course_home then "#{base_url}/home/#{course_id}"
    when :assignment then "#{base_url}/common/dialogs/quickLink/quickLink.d2l?ou=#{course_id}&type=dropbox&id=#{id}"
    when :quiz then "#{base_url}/common/dialogs/quickLink/quickLink.d2l?ou=#{course_id}&type=quiz&id=#{id}"
    else "#{base_url}/home/#{course_id}"
    end
  end

  def ensure_array(data)
    if data.is_a?(Array) then data
    elsif data.is_a?(Hash) then data['Objects'] || data['Items'] || []
    else []
    end
  end

  def do_get(path, force_refresh: false)
    # Normalize path (e.g. remove multiple slashes)
    path = path.gsub('//', '/')
    
    cache_record = ApiCache.active.find_by(path: path) || ApiCache.find_by(path: path)
    cached_data = JSON.parse(cache_record.data) rescue nil
    
    is_fresh = cache_record && !cache_record.is_archived && (Time.current - cache_record.updated_at < 600)

    # 1. If we have fresh data, just return it
    return cached_data if !force_refresh && cached_data && is_fresh

    # 2. If data exists but stale, queue a background update if authenticated
    if !force_refresh && cached_data && authenticated?
      unless @pending_tasks.key?(path)
        @pending_tasks[path] = true
        @task_queue << { path: path }
      end
      return cached_data
    end

    # 3. If offline or not authenticated, fall back to whatever cache we have
    return cached_data if !authenticated? && cached_data

    # 4. Otherwise, perform a synchronous fetch
    fetch_and_cache(path)
  end

  def cleanup_cache
    # Delete inactive/archived cache older than 3 days
    count = ApiCache.where("is_archived = ? AND updated_at < ?", true, 3.days.ago).delete_all
    # Delete any cache entries older than 7 days regardless of status
    count += ApiCache.where("updated_at < ?", 7.days.ago).delete_all
    puts "[API Cache] Cleaned up #{count} old records." if count > 0
  end

  def do_post(path, body_data)
    return nil unless authenticated?
    uri = URI("https://#{@host}#{path}")
    request = Net::HTTP::Post.new(uri)
    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string
    end
    request.content_type = 'application/json'
    request.body = body_data.to_json

    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true
    
    begin
      response = http.request(request)
      response.code.start_with?('2')
    rescue
      false
    end
  end

  def dismiss_news_item(course_id, news_id)
    do_post("/d2l/api/le/#{@api_version}/#{course_id}/news/#{news_id}/dismiss", {})
  end

  def mark_notification_read(notification_id)
    do_post("/d2l/api/lp/#{@api_version}/notifications/#{notification_id}/read", {})
  end

  private

  def read_cache(path)
    @cache_memory ||= {}
    return @cache_memory[path] if @cache_memory.key?(path)

    raw_data = ApiCache.active.where(path: path).pick(:data)
    @cache_memory[path] = JSON.parse(raw_data) rescue nil
  end

  def write_cache(path, data)
    return unless data.is_a?(Hash) || data.is_a?(Array)
    return if data.is_a?(Hash) && (data.key?('Errors') || data.key?('ErrorMessage'))
    
    @cache_memory ||= {}
    @cache_memory[path] = data

    cache = ApiCache.find_or_initialize_by(path: path)
    cache.data = data.to_json
    cache.is_archived = false
    cache.save!
  end

  def archive_cache(path)
    @cache_memory ||= {}
    @cache_memory.delete(path)
    ApiCache.where(path: path).update_all(is_archived: true, updated_at: Time.current)
  end

  def fetch_and_cache(path)
    return nil unless authenticated?
    return read_cache(path) if @degraded_mode
    
    # Use a mutex per path to prevent multiple threads from fetching the same URL
    lock = @sync_lock.synchronize { @in_flight_requests[path] ||= Mutex.new }
    
    lock.synchronize do
      # Check if another thread just finished fetching this while we were waiting
      # but only if we weren't explicitly told to force refresh (which fetch_and_cache usually is)
      # Actually, do_get calls this when it needs it fresh.
      
      uri = URI("https://#{@host}#{path}")
      request = Net::HTTP::Get.new(uri)
      if @token
        request['Authorization'] = "Bearer #{@token}"
      elsif @cookie_string
        request['Cookie'] = @cookie_string
      end
      request['Accept'] = 'application/json'

      http = Net::HTTP.new(uri.host, uri.port)
      http.use_ssl = true
      http.read_timeout = 15
      
      begin
        puts "[API Cache] Fetching: #{path}"
      response = http.request(request)
      if response.code.start_with?('2')
        @degraded_mode = false
        @auth_notification_sent = false
        data = JSON.parse(response.body)
        write_cache(path, data)
        data
      elsif ['401', '403'].include?(response.code)
        handle_auth_failure(response.code, path)
        read_cache(path)
      else
        read_cache(path)
      end
    rescue
      read_cache(path)
      ensure
        @sync_lock.synchronize { @in_flight_requests.delete(path) }
      end
    end
  end

  def handle_auth_failure(code, path = nil)
    # If it's a 401, it's definitely an authentication issue
    if code == '401'
      @degraded_mode = true
      @auth_notification_sent = true
      Brilliant::EventBus.publish(:authentication_failure, { code: code, host: @host })
      return
    end

    # For 403s, verify if the session is truly dead using the "whoami" probe
    safe_path = "/d2l/api/lp/#{@api_version}/users/whoami"
    if path == safe_path
      @degraded_mode = true
      @auth_notification_sent = true
      Brilliant::EventBus.publish(:authentication_failure, { code: code, host: @host })
      return
    end

    # Light-weight probe to distinguish between session expiry and restricted content
    begin
      uri = URI("https://#{@host}#{safe_path}")
      request = Net::HTTP::Get.new(uri)
      if @token
        request['Authorization'] = "Bearer #{@token}"
      elsif @cookie_string
        request['Cookie'] = @cookie_string
      end

      probe_res = Net::HTTP.start(uri.hostname, uri.port, use_ssl: true, read_timeout: 5) { |http| http.request(request) }
      if probe_res.code.start_with?('2')
        puts "[Auth] 403 suppressed for #{path} - Session still valid via whoami probe."
        return
      end
    rescue => e
      puts "[Auth] Probe failed during 403 handling: #{e.message}"
    end

    @degraded_mode = true
    @auth_notification_sent = true
    Brilliant::EventBus.publish(:authentication_failure, { code: code, host: @host })
  end
end
