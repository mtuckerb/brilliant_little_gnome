require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'
require 'time'

class BrilliantClient
  class AuthenticationError < StandardError; attr_reader :status_code; def initialize(msg, code); super(msg); @status_code = code; end; end
  
  attr_accessor :token, :cookie_string, :host, :user_display_name, :sync_status, :degraded_mode

  def initialize
    data_dir = ENV['BRILLIANT_DATA_DIR'] || '.'
    @config_path = File.join(data_dir, 'config', 'connection.json')
    load_connection_config
    
    @api_version = "1.40"
    @sync_lock = Mutex.new
    @syncing = false
    @sync_status = { status: "idle", progress: 0, current_task: nil }
    @degraded_mode = false
    
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
        @sync_lock.synchronize { @syncing = true }
        @sync_status = { status: "syncing", progress: 0, current_task: "Starting proactive sync..." }
        
        begin
          # Check for a "full sync" request
          full_sync = UserPreference.get('force_full_sync') == 'true'
          
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
            sync_assignments(course_id, assignments) if assignments
            archive_cache(assign_path) if assignments

            sleep 0.1
            @sync_status[:current_task] = "#{short_name} - Syncing Quizzes..."
            quiz_path = "/d2l/api/le/#{@api_version}/#{course_id}/quizzes/"
            quizzes = get_quizzes(course_id)
            sync_quizzes(course_id, quizzes) if quizzes
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

  def sync_notifications(courses, user, full_sync: false)
    puts "[Brilliant API] Syncing notifications to DB..."
    
    last_sync_key = "last_notification_sync_at"
    last_sync_time = full_sync ? nil : UserPreference.get(last_sync_key)
    since_param = last_sync_time ? "?since=#{URI.encode_www_form_component(last_sync_time)}" : ""
    
    # Sync courses to normalized table
    ActiveRecord::Base.transaction do
      courses.each do |c|
        next if c.nil? || c['OrgUnit'].nil?

        # Intelligent merge: skip if the response for the course name/code seems degraded
        new_name = c['OrgUnit']['Name']
        new_code = c['OrgUnit']['Code']
        next if new_name.nil? || new_name.empty?

        course = Course.find_or_initialize_by(org_unit_id: c['OrgUnit']['Id'].to_s)
        
        # Protection: Don't overwrite robust name with numeric ID
        course.name = new_name if new_name.present? && !new_name.match?(/^\d+$/)
        course.code = c['OrgUnit']['Code']
        course.is_pinned = !c['PinDate'].nil?
        course.last_accessed_at = (Time.parse(c.dig('Access', 'LastAccessed')) rescue nil)
        course.semester = extract_semester_from_name(course.name)
        
        # Image/Banner URL extraction
        # D2L LP Enrollment Object usually has an 'OrgUnit' -> 'ImageUrl' if it's there
        # but modern versions often store it in a nested Display object
        img_url = c.dig('OrgUnit', 'ImageUrl') || c.dig('OrgUnit', 'Image', 'ViewUrl') || c.dig('OrgUnit', 'Image', 'DisplayUrl')
        if img_url && !img_url.empty?
          # Ensure absolute URL for Electron UI
          img_url = "https://#{@host}#{img_url}" if img_url.start_with?("/")
          course.banner_url = img_url
        end
        
        course.save!
      end
    end
    # archive enrollments cache after syncing to course table
    archive_cache("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/")

    # Get unified feed first (broad coverage)
    feed_path = "/d2l/api/lp/#{@api_version}/feed/"
    feed_path += "?since=#{URI.encode_www_form_component(last_sync_time)}" if last_sync_time
    feed_items = get_unified_feed(courses, since: last_sync_time)
    ActiveRecord::Base.transaction do
      feed_items.each do |item|
        upsert_notification(item)
      end
    end
    archive_cache(feed_path)

    # Get per-course News/Announcements explicitly
    courses.take(full_sync ? courses.size : 15).each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      if course_id.nil?
        puts "[Brilliant API] Skipping course with nil OrgUnit Id: #{c.inspect}"
        next
      end
      
      # News/Announcements
      news_path = "/d2l/api/le/#{@api_version}/#{course_id}/news/"
      news_path += "?since=#{URI.encode_www_form_component(last_sync_time)}" if last_sync_time
      news_data = get_news(course_id, since: last_sync_time)
      items = if news_data.is_a?(Array)
                news_data
              elsif news_data.is_a?(Hash)
                news_data['Items'] || []
              else
                []
              end
      
      ActiveRecord::Base.transaction do
        items.each do |item|
          # Don't overwrite with empty body if we already have one
          existing = Notification.find_by(external_id: "news_#{course_id}_#{item['Id']}", course_id: course_id.to_s)
          new_body = item.dig('Summary', 'Text') || item.dig('Body', 'Text')
          next if existing && (new_body.nil? || new_body.empty?)

          upsert_notification({
            id: "news_#{course_id}_#{item['Id']}",
            type: 'News',
            title: item['Title'],
            body: item.dig('Summary', 'Text') || item.dig('Body', 'Text'),
            date: item['StartDate'] || item['LastModifiedDate'] || item['CreatedDate'],
            course_id: course_id,
            course_name: c['OrgUnit']['Name'],
            urgency: 1,
            attachments: item['Attachments']&.to_json,
            is_personal: false,
            url: "/course/#{course_id}/announcements"
          })
        end

        # Persist news attachments
        items.each do |item|
          if item['Attachments'] && !item['Attachments'].empty?
            item['Attachments'].each do |att|
              persist_attachment(course_id, "news/#{item['Id']}/attachments/#{att['FileId']}", att['FileId'], att['FileName'])
            end
          end
        end
        archive_cache(news_path)

        # Course Overview (requested specifically by user)
        overview_path = "/d2l/api/le/#{@api_version}/#{course_id}/overview"
        overview = get_overview(course_id)
        if overview && (overview['Description']&.fetch('Text', nil) || overview['Title'])
          upsert_notification({
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
          })
          archive_cache(overview_path)
        end
      end
    end

    # Get content updates
    content_items = get_content_notifications(courses, since: last_sync_time)
    ActiveRecord::Base.transaction do
      content_items.each do |item|
        upsert_notification(item)
      end
    end

    # Get grades
    grade_items = get_recent_grades_notifications(courses) # This one is harder to "since"
    ActiveRecord::Base.transaction do
      grade_items.each do |item|
        upsert_notification(item)
      end
    end

    # Get discussions
    disc_items = get_discussion_notifications(courses, user['Identifier'])
    ActiveRecord::Base.transaction do
      disc_items.each do |item|
        upsert_notification(item)
      end
    end

    # Sync Upcoming Assignments as Notifications
    sync_upcoming_assignment_notifications(courses)
    
    UserPreference.set(last_sync_key, Time.now.utc.iso8601)

    puts "[Brilliant API] Notification sync complete."
  end

  def sync_upcoming_assignment_notifications(courses)
    puts "[Brilliant API] Syncing upcoming assignments to notifications..."
    
    # We look for assignments due in the next 7 days
    upcoming_limit = Time.now + 7.days
    
    ActiveRecord::Base.connection_pool.with_connection do
      Assignment.where("due_date > ? AND due_date <= ?", Time.now, upcoming_limit).each do |a|
        next if a.nil?
        course = courses.find { |c| c['OrgUnit']['Id'].to_s == a.course_id.to_s }
        course_name = course ? course['OrgUnit']['Name'] : (Course.find_by(org_unit_id: a.course_id)&.name || "Unknown Course")

        type_label = a.assignment_type == 'quiz' ? 'Quiz' : 'Assignment'
        url = a.assignment_type == 'quiz' ? 
              "/course/#{a.course_id}/quizzes/#{a.brightspace_id.sub('quiz_', '')}" : 
              "/course/#{a.course_id}/assignments/#{a.brightspace_id}"
        
        upsert_notification({
          id: "upcoming_assignment_#{a.brightspace_id}",
          type: 'Assignment',
          title: "Upcoming #{type_label}: #{a.name}",
          body: "This #{type_label.downcase} is due on #{a.due_date.strftime('%A, %b %d at %I:%M %p')}.",
          date: a.due_date - 1.day, # Set notification date to roughly now/recent to show in feed
          course_id: a.course_id,
          course_name: course_name,
          urgency: 2,
          is_personal: true,
          url: url
        })
      end
    end
  end

  def get_content_notifications(courses, since: nil)
    all_updates = []
    # Only check recent courses to avoid long sync
    courses.take(20).each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      
      path = "/d2l/api/le/#{@api_version}/#{course_id}/content/updates"
      path += "?since=#{URI.encode_www_form_component(since)}" if since
      
      data = do_get(path)
      next unless data

      items = data.is_a?(Array) ? data : (data['Items'] || [])
      items.each do |item|
        all_updates << {
          id: "content_#{course_id}_#{item['Identifier'] || item['Id']}",
          type: 'Content',
          title: "Content Updated: #{item['Title']}",
          body: "New or updated content in #{c['OrgUnit']['Name']}: #{item['Title']}",
          # prioritize CreatedDate/LastModifiedDate
          date: item['CreatedDate'] || item['LastModifiedDate'] || Time.now.iso8601,
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

  def create_system_notification(data)
    # Ensure we have a database connection in case this is called from a thread
    ActiveRecord::Base.connection_pool.with_connection do
      upsert_notification({
        id: data[:id],
        type: 'System',
        title: data[:title],
        body: data[:body],
        date: Time.now.iso8601,
        course_id: 'SYSTEM',
        course_name: 'Brilliant System',
        urgency: data[:urgency] || 2,
        is_personal: true,
        url: '/'
      })
    end
  rescue => e
    puts "[!] Failed to create system notification: #{e.message}"
  end

  def upsert_notification(data)
    # Map symbols to strings for ActiveRecord
    n = Notification.find_or_initialize_by(external_id: data[:id].to_s, course_id: data[:course_id].to_s)
    
    # Intelligent protection: don't overwrite existing title/body with thinner data
    new_title = data[:title]
    new_body = data[:body]

    n.notification_type = data[:type]
    n.title = new_title if new_title.present?
    n.body = new_body if new_body.present?

    # Improved date handling: handle strings, Time objects, and nil
    raw_date = data[:date]
    begin
      if raw_date
        parsed_date = if raw_date.is_a?(Time)
                        raw_date
                      else
                        Time.parse(raw_date.to_s)
                      end
        
        # Only update date if it's a new record OR if the date is significantly different
        # (to avoid constant timestamp shifting for items without stable API dates)
        if n.new_record? || !n.date || (parsed_date - n.date).abs > 60
          n.date = parsed_date
        end
      else
        n.date ||= Time.now
      end
    rescue => e
      puts "[Brilliant API] Date parse error for #{data[:id]}: #{e.message} (Raw: #{raw_date})"
      n.date ||= Time.now
    end
    
    n.course_name = data[:course_name]
    
    # Auto-extract semester if not provided
    if (n.semester.nil? || n.semester.to_s.empty?) && n.course_name
      n.semester = extract_semester_from_name(n.course_name)
    end
    if data[:semester]
        n.semester = data[:semester]
    end

    n.attachments = data[:attachments] if data[:attachments]

    n.urgency = data[:urgency]
    n.is_personal = data[:is_personal]
    n.url = data[:url]
    n.save!
  end

  def extract_semester_from_name(full_name)
    return nil unless full_name

    # Try to find something like (2025 Spring) or (Spring 2025)
    # Handle cases like: "Course Name (2025 Fall)", "Other Course (Spring 2026)"
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

  def load_cookies_from_file
    content = File.read('cookies.txt').strip
    return if content.empty?

    if content.start_with?('ey')
      @token = content
    else
      # Strip "Cookie: " prefix if present from browser copy-paste
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
    data = do_get("/d2l/api/lp/#{@api_version}/users/whoami")
    @user_display_name = data['DisplayName'] if data
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
        access_time = raw_access ? Time.parse(raw_access) : Time.at(0)
      rescue ArgumentError
        access_time = Time.at(0)
      end
      [pin_score, -access_time.to_i]
    end
  end

  def get_toc(org_unit_id)
    data = do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/content/toc")
    # Always normalize to a hash with 'Modules' for view/sync consistency
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
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/#{assignment_id}/feedback/rubrics/")
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
      topics.each do |t| 
        t['ForumId'] = f['ForumId']
        t['ForumName'] = f['Name']
      end
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
    data = do_get(path, force_refresh: force_refresh)
    
    # Fallback: if threads endpoint 404s or is empty, try to synthesize from posts
    if data.nil? || (data.is_a?(Hash) && (data['Items'] || []).empty?)
      puts "[Brilliant API] Threads 404 or empty for topic #{topic_id}. Attempting synthesis from posts..."
      posts_data = get_topic_posts(org_unit_id, forum_id, topic_id)
      posts = posts_data.is_a?(Hash) ? (posts_data['Items'] || []) : posts_data
      
      if posts && posts.any?
        # Group by ThreadId and take the "first" post (the one with ParentPostId nil) as the thread representative
        threads = synthesize_threads_from_posts(posts)
        return { 'Items' => threads }
      end
    end
    
    data
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

  def synthesize_threads_from_posts(posts)
    # Find posts that effectively started a thread (either ParentPostId is nil or they are the earliest post with a specific ThreadId)
    # Actually, if we have ThreadId, we can group by it.
    threads_map = {}
    
    posts.each do |p|
      tid = p['ThreadId']
      # We want the 'root' post of the thread
      if p['ParentPostId'].nil?
        threads_map[tid] = {
          'ThreadId' => tid,
          'Subject' => p['Subject'],
          'Title' => p['Subject'],
          'PostingUserDisplayName' => p['PostingUserDisplayName'],
          'LastModified' => p['DatePosted'],
          'DatePosted' => p['DatePosted'],
          'IsPinned' => false # Best guess
        }
      end
    end
    threads_map.values
  end

  def get_all_news(courses)
    news_items = []
    courses.each do |c|
      items_data = get_news(c['OrgUnit']['Id'])
      items = items_data.is_a?(Hash) ? (items_data['Items'] || []) : (items_data || [])
      if items && items.is_a?(Array)
        items.each { |i| i['CourseName'] = c['OrgUnit']['Name']; i['CourseId'] = c['OrgUnit']['Id'] }
        news_items.concat(items)
      end
    end
    news_items.sort_by { |i| i['StartDate'] || "" }.reverse.take(10)
  end

  def get_news(org_unit_id, since: nil)
    path = "/d2l/api/le/#{@api_version}/#{org_unit_id}/news/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    
    do_get(path)
  end

  def get_unified_feed(courses = [], since: nil)
    # Used for title mapping
    course_map = courses.each_with_object({}) { |c, h| h[c['OrgUnit']['Id'].to_s] = c['OrgUnit']['Name'] }

    path = "/d2l/api/lp/#{@api_version}/feed/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    
    feed_data = do_get(path)
    feed = if feed_data.is_a?(Array)
             feed_data
           elsif feed_data.is_a?(Hash)
             feed_data['Items'] || []
           else
             []
           end
    
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
    modules = toc.is_a?(Hash) ? (toc['Modules'] || []) : toc
    puts "[Sync] Beginning sync for #{modules.size} modules in course #{course_id}"
    return unless modules.is_a?(Array)
    
    ActiveRecord::Base.transaction do
      modules.each_with_index do |mod, index|
        begin
          m = ContentModule.find_or_initialize_by(brightspace_id: mod['ModuleId'].to_s, course_id: course_id.to_s)
          m.title = mod['Title']
          new_desc = mod.dig('Description', 'Html') || mod.dig('Description', 'Text')
          m.description = new_desc if new_desc && !new_desc.empty?
          m.sort_order = index
          m.save!
          
          # Sync topics/items
          (mod['Topics'] || []).each_with_index do |topic, t_index|
            # Correctly handle both Identifier and TopicId
            t_id = (topic['Identifier'] || topic['TopicId'] || topic['Id']).to_s
            item = ContentItem.find_or_initialize_by(brightspace_id: t_id, module_id: mod['ModuleId'].to_s)
            item.title = topic['Title']
            item.item_type = (topic['TypeIdentifier'] || topic['Type']).to_s
            item.url = topic['Url']
            item.is_hidden = topic['IsHidden'] || false
            item.sort_order = t_index
            item.save!

            # Persist metadata for URLs/attachments
            item.attachments = [topic].to_json if topic.any?
            item.save! if topic.any?

            # Persist content file if it's a direct resource
            if item.url && (item.url.start_with?('/content/enforced/') || item.url.include?('/viewContent/'))
              Thread.new(course_id, item.brightspace_id, item.title) do |cid, tid, title|
                ActiveRecord::Base.connection_pool.with_connection do
                  persist_attachment(cid, "content/topics/#{tid}/file", tid, title)
                end
              end
            end
          end
          
          # Recursively sync sub-modules
          sync_sub_modules(course_id, mod['ModuleId'].to_s, mod['Modules']) if mod['Modules']
        rescue => e
          puts "[Sync] Error syncing module #{mod['ModuleId']}: #{e.message}"
        end
      end
    end
  end

  def sync_sub_modules(course_id, parent_id, sub_modules)
    modules = sub_modules.is_a?(Hash) ? (sub_modules['Modules'] || []) : sub_modules
    return unless modules.is_a?(Array)
    
    modules.each_with_index do |mod, index|
      begin
        m = ContentModule.find_or_initialize_by(brightspace_id: mod['ModuleId'].to_s, course_id: course_id.to_s)
        m.title = mod['Title']
        new_desc = mod.dig('Description', 'Html') || mod.dig('Description', 'Text')
        m.description = new_desc if new_desc && !new_desc.empty?
        m.sort_order = index
        m.parent_id = parent_id
        m.save!
        
        (mod['Topics'] || []).each_with_index do |topic, t_index|
          t_id = (topic['Identifier'] || topic['TopicId'] || topic['Id']).to_s
          item = ContentItem.find_or_initialize_by(brightspace_id: t_id, module_id: mod['ModuleId'].to_s)
          item.title = topic['Title']
          item.item_type = (topic['TypeIdentifier'] || topic['Type']).to_s
          item.url = topic['Url']
          item.is_hidden = topic['IsHidden'] || false
          item.sort_order = t_index
          item.save!

          item.attachments = [topic].to_json if topic.any?
          item.save! if topic.any?

          if item.url && (item.url.start_with?('/content/enforced/') || item.url.include?('/viewContent/'))
            Thread.new(course_id, item.brightspace_id, item.title) do |cid, tid, title|
              ActiveRecord::Base.connection_pool.with_connection do
                persist_attachment(cid, "content/topics/#{tid}/file", tid, title)
              end
            end
          end
        end
        
        sync_sub_modules(course_id, mod['ModuleId'].to_s, mod['Modules']) if mod['Modules']
      rescue => e
        puts "[Sync] Error syncing sub-module #{mod['ModuleId']}: #{e.message}"
      end
    end
  end

  def sync_assignments(course_id, assignments)
    items = ensure_array(assignments)
    
    return if items.empty?

    # Process each assignment - we fetch full details if possible to get attachments/instructions
    items.each do |a_summary|
      next if a_summary.nil?
      t_id = (a_summary['Id'] || a_summary['Identifier'] || a_summary['TopicId']).to_s
      
      # Try to get full details (cached or fetch)
      a = get_assignment(course_id, t_id) || a_summary
      
      next if a.nil?
      assignment = Assignment.find_or_initialize_by(brightspace_id: t_id, course_id: course_id.to_s)
      
      # Protection: Don't overwrite robust name with thin data (archived courses sometimes return numeric IDs as names)
      new_name = a['Name']
      assignment.name = new_name if new_name.present? && !new_name.match?(/^\d+$/)
      
      new_due = (Time.parse(a['DueDate'] || a['DueDate']) rescue nil)
      assignment.due_date = new_due if new_due
      
      new_desc = a.dig('CustomInstructions', 'Text') || a.dig('Description', 'Text')
      assignment.description = new_desc if new_desc.present?
      
      # Capture attachments and external URLs
      atts = (a['Attachments'] || []) + (a['LinkAttachments'] || [])
      assignment.attachments = atts.to_json if atts.any?
      
      # Proactively persist binary attachments in background
      if a['Attachments']
        Thread.new(course_id, t_id, a['Attachments']) do |cid, aid, attachments|
          ActiveRecord::Base.connection_pool.with_connection do
            attachments.each do |att|
              persist_attachment(cid, "dropbox/folders/#{aid}/attachments/#{att['FileId']}", att['FileId'], att['FileName'])
            end
          end
        end
      end

      assignment.is_graded = a['IsGraded'] || false
      assignment.external_url = a.dig('CustomInstructions', 'Html')&.match(/href="([^"]+)"/)&.[](1) if assignment.external_url.nil?
      
      assignment.grade_item_id = a['GradeItemId'].to_s if a['GradeItemId']
      assignment.save!
    end
  end

  def sync_quizzes(course_id, quizzes)
    items = ensure_array(quizzes)
    
    return if items.empty?

    items.each do |q|
      next if q.nil?
      q_id = (q['QuizId'] || q['Id'] || q['Identifier']).to_s
      # We use the Assignment model for Quizzes too, but mark the type
      assignment = Assignment.find_or_initialize_by(brightspace_id: "quiz_#{q_id}", course_id: course_id.to_s)
      
      assignment.name = q['Name']
      assignment.assignment_type = 'quiz'
      
      # Quizzes use DueDate in their object format
      new_due = (Time.parse(q['DueDate']) rescue nil)
      assignment.due_date = new_due if new_due
      
      new_desc = q.dig('Description', 'Text') || q.dig('Header', 'Text')
      assignment.description = new_desc if new_desc.present?
      
      # Quizzes can have link attachments too
      atts = (q['LinkAttachments'] || [])
      assignment.attachments = atts.to_json if atts.any?

      assignment.is_graded = q['IsActive'] || false # Quizzes don't have IsGraded in same way, but usually active means gradable
      assignment.save!
    end
  end

  def sync_discussions(course_id, forums)
    return unless forums.is_a?(Array)
    
    forums.each do |f|
      next if f.nil?
      f_id = (f['ForumId'] || f['Id'] || f['Identifier']).to_s
      forum = DiscussionForum.find_or_initialize_by(brightspace_id: f_id, course_id: course_id.to_s)
      forum.name = f['Name'] if f['Name']
      new_desc = f.dig('Description', 'Text')
      forum.description = new_desc if new_desc && !new_desc.empty?
      forum.save!
      
      topics_data = get_discussion_topics(course_id, f['ForumId'])
      topics = ensure_array(topics_data)
      sync_discussion_topics(course_id, f_id, topics) if topics
    end
  end

  def sync_discussion_topics(course_id, forum_id, topics)
    return unless topics.is_a?(Array)
    
    topics.each_with_index do |t, index|
      next if t.nil?
      t_id = (t['TopicId'] || t['Id'] || t['Identifier']).to_s
      topic = DiscussionTopic.find_or_initialize_by(brightspace_id: t_id, forum_id: forum_id.to_s)
      topic.course_id = course_id.to_s
      topic.name = t['Name']
      new_desc = t.dig('Description', 'Html') || t.dig('Description', 'Text')
      topic.description = new_desc if new_desc && !new_desc.empty?
      topic.sort_order = index
      
      # D2L API can be inconsistent with count keys
      topic.thread_count = t['ThreadCount'] || t['TotalThreads'] || t['Threads'] || 0
      topic.post_count = t['PostCount'] || t['TotalPosts'] || t['Posts'] || 0
      
      lpd = t['LastPostDate'] || t['LastPost']&.fetch('DatePosted', nil)
      topic.last_post_date = (Time.parse(lpd.to_s) rescue nil) if lpd
      
      topic.save!
      
      # We don't sync threads here by default as it's too heavy for a broad sync,
      # but we provide the method for specific topic refreshes.
    end
  end

  def sync_topic_posts(course_id, forum_id, topic_id, posts)
    return unless posts.is_a?(Array)
    
    ActiveRecord::Base.transaction do
      posts.each do |p|
        next if p.nil?
        p_id = (p['PostId'] || p['Id'] || p['Identifier']).to_s
        post = DiscussionPost.find_or_initialize_by(
          brightspace_id: p_id,
          topic_id: topic_id.to_s,
          thread_id: p['ThreadId'].to_s
        )
        post.parent_post_id = p['ParentPostId'].to_s if p['ParentPostId']
        post.subject = p['Subject']
        
        new_body = p.dig('Body', 'Html') || p.dig('Body', 'Text') || (p['Body'].is_a?(Hash) ? p.dig('Body', 'Html') : p['Body'])
        post.body = new_body if new_body && !new_body.empty?
        
        post.author_name = p['PostingUserDisplayName']
        new_posted = (Time.parse(p['DatePosted']) rescue nil)
        post.posted_at = new_posted if new_posted
        # Check for instructor role
        post.is_instructor = p.dig('Author', 'IsInstructor') == true || p.dig('Author', 'RoleName') =~ /Instructor/i rescue false
        
        post.save!
      end
    end

    # After syncing posts, update the cached counts on the topic record itself
    # for faster rendering in lists.
    topic = DiscussionTopic.find_by(brightspace_id: topic_id.to_s)
    if topic
      topic.thread_count = DiscussionPost.where(topic_id: topic_id.to_s).distinct.count(:thread_id)
      topic.post_count = DiscussionPost.where(topic_id: topic_id.to_s).count
      topic.last_post_date = DiscussionPost.where(topic_id: topic_id.to_s).maximum(:posted_at)
      topic.save!
    end
  end

  def sync_discussion_thread(course_id, topic_id, thread)
    return if thread.nil?

    th_id = (thread['ThreadId'] || thread['Id'] || thread['Identifier']).to_s
    t = DiscussionThread.find_or_initialize_by(brightspace_id: th_id, topic_id: topic_id.to_s)
    t.course_id = course_id.to_s
    t.subject = thread['Subject'] || thread['Title']
    
    new_body = thread.dig('Description', 'Text') || thread.dig('Body', 'Text')
    t.body = new_body if new_body && !new_body.empty?
    
    t.author_name = thread['PostingUserDisplayName'] || thread.dig('LastPost', 'UserDisplayName')
    new_posted = (Time.parse(thread['DatePosted'] || thread['LastModified']) rescue nil)
    t.posted_at = new_posted if new_posted
    t.is_pinned = thread['IsPinned'] || false
    t.unread_count = thread['UnreadReplyCount'] || 0
    t.save!
  end

  def sync_grades(course_id, grade_values)
    return unless grade_values.is_a?(Array)
    
    # FETCH ASSIGNMENTS FIRST!
    # We need assignments synced to establish the grade_item_id -> due_date mapping
    assignments_raw = get_assignments(course_id)
    sync_assignments(course_id, assignments_raw) if assignments_raw

    # Fetch Grade Object Metadata to get Weights
    # /d2l/api/le/(version)/(orgUnitId)/grades/ returns all grade objects
    metadata_path = "/d2l/api/le/#{@api_version}/#{course_id}/grades/"
    metadata_raw = do_get(metadata_path)
    metadata = ensure_array(metadata_raw)
    
    weights_map = {}
    metadata.each do |m|
      weights_map[(m['Id'] || m['Identifier']).to_s] = m['Weight']
    end

    ActiveRecord::Base.transaction do
      grade_values.each do |g|
        next if g.nil?
        obj_id = (g['GradeObjectIdentifier'] || g['Identifier'] || g['Id']).to_s
        grade = Grade.find_or_initialize_by(brightspace_id: obj_id, course_id: course_id.to_s)
        
        # Protection: archived courses often return numeric names
        new_name = g['GradeObjectName']
        grade.name = new_name if new_name.present? && !new_name.match?(/^\d+$/)
        
        # Use .present? to avoid wiping robust grades or comments with empty strings/nil
        new_displayed = g['DisplayedGrade']
        grade.displayed_grade = new_displayed if new_displayed.present?
        
        grade.numerator = g['PointsNumerator'] || g['Numerator'] if g['PointsNumerator'] || g['Numerator']
        grade.denominator = g['PointsDenominator'] || g['Denominator'] if g['PointsDenominator'] || g['Denominator']
        
        new_weight = weights_map[obj_id] || g.dig('Weight')
        grade.weight = new_weight if new_weight
        
        grade.grade_object_type = g['GradeObjectType']
        new_mod = (Time.parse(g['LastModified']) rescue nil)
        grade.last_modified = new_mod if new_mod
        
        new_comments = g.dig('Comments', 'Html') || g.dig('Comments', 'Text')
        grade.comments = new_comments if new_comments.present?
        
        # Link Due Date from Assignment if possible
        assignment = Assignment.find_by(course_id: course_id.to_s, grade_item_id: obj_id)
        grade.due_date = assignment.due_date if assignment
        
        grade.save!
      end
    end
  end

  def get_recent_grades_notifications(courses)
    alerts = []
    courses.each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      grades_raw = get_grades(course_id)
      grades = if grades_raw.is_a?(Array)
                 grades_raw
               elsif grades_raw.is_a?(Hash)
                 grades_raw['Items'] || []
               else
                 []
               end
      grades.each do |g|
        next unless g['DisplayedGrade']
        
        alerts << {
          id: "grade_#{course_id}_#{g['GradeObjectIdentifier']}",
          type: 'Grade',
          title: "Grade Updated: #{g['GradeObjectName']}",
          body: "Your grade for #{g['GradeObjectName']} is now #{g['DisplayedGrade']}.",
          date: nil, # Will fallback to Time.now on first creation only
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
    notifications = []
    relevant_courses = courses.take(10) 

    relevant_courses.each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      topics = get_all_topics(course_id) || []
      
      topics.each do |t|
        # Only check if unread counts exist to minimize storming
        # (Though we'll need to fetch threads to check for personal involvement)
        threads_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/#{t['ForumId']}/topics/#{t['TopicId']}/threads/"
        threads_data = do_get(threads_path)
        
        threads = ensure_array(threads_data)
        
        threads.each do |th|
          next if (th['UnreadReplyCount'] || 0) == 0
          
          posts_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/#{t['ForumId']}/topics/#{t['TopicId']}/threads/#{th['ThreadId']}/posts/"
          posts_data = do_get(posts_path)
          posts = ensure_array(posts_data)
          
          user_involved = posts.any? { |p| p['PostingUserDisplayName'] == @user_display_name }
          user_post_ids = posts.select { |p| p['PostingUserDisplayName'] == @user_display_name }.map { |p| p['PostId'].to_s }
          replies_to_user = posts.select { |p| user_post_ids.include?(p['ParentPostId'].to_s) }
          
          urgency = 1
          is_personal = false
          title = "Unread Posts in: #{th['Subject'] || th['Title']}"

          if !replies_to_user.empty?
             urgency = 3
             is_personal = true
             title = "Direct Reply in: #{th['Subject'] || th['Title']}"
          elsif user_involved
             urgency = 2
             is_personal = true
             title = "New Activity in your Thread: #{th['Subject'] || th['Title']}"
          end
          
          notifications << {
            id: "disc_#{th['ThreadId']}",
            type: 'Discussion',
            title: title,
            body: "There are #{th['UnreadReplyCount']} unread replies in #{t['Name']}.",
            date: th['LastModified'] || th['DatePosted'],
            course_id: course_id,
            course_name: c['OrgUnit']['Name'],
            urgency: urgency,
            is_personal: is_personal,
            url: "/course/#{course_id}/discussions/#{t['ForumId']}/topics/#{t['TopicId']}/threads/#{th['ThreadId']}"
          }
        end
      end
    end
    notifications
  end

  def get_overview(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/overview")
  end

  def download_file(path, limit = 5)
    return nil unless authenticated?
    raise "Too many redirects" if limit == 0
    
    # Handle absolute URLs gracefully
    if path.start_with?('http')
      uri = URI.parse(path)
    else
      path = "/#{path}" unless path.start_with?('/')
      uri = URI("https://#{@host}#{path}")
    end

    request = Net::HTTP::Get.new(uri)
    
    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string
    end

    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true
    
    begin
      response = http.request(request)
      if response.code == '302' || response.code == '301' || response.code == '307' || response.code == '308'
        location = response['location']
        # If redirect is relative, join it with current uri
        new_uri = URI.join(uri.to_s, location)
        return download_file(new_uri.to_s, limit - 1)
      end
      response
    rescue => e
      puts "Download connection error for #{path}: #{e.message}"
      nil
    end
  end

  def persist_attachment(course_id, api_path, file_id, file_name)
    # api_path is something like "dropbox/folders/123/attachments/456"
    path = api_path.start_with?('/') ? api_path : "/d2l/api/le/#{@api_version}/#{course_id}/#{api_path}"
    
    # Create local storage path
    data_dir = ENV['BRILLIANT_DATA_DIR'] || '.'
    local_dir = File.join(data_dir, 'public', 'attachments', course_id.to_s)
    FileUtils.mkdir_p(local_dir)
    
    safe_name = file_name.to_s.gsub(/[^0-9A-Za-z.\- ]/, '_')
    local_path = File.join(local_dir, "#{file_id}_#{safe_name}")
    
    return local_path if File.exist?(local_path)

    response = download_file(path)
    if response && response.code == '200'
      begin
        File.open(local_path, 'wb') { |f| f.write(response.body) }
        return local_path
      rescue => e
        puts "[Brilliant API] Failed to persist attachment: #{e.message}"
      end
    end
    nil
  end

  def portal_url_for(type, options = {})
    course_id = options[:course_id]
    id = options[:id]
    
    base_url = "https://#{@host}/d2l"
    
    case type
    when :course_home
      "#{base_url}/home/#{course_id}"
    when :assignment
      # Uses QuickLink to handle auth context better and avoid 403s on certain courses
      "#{base_url}/common/dialogs/quickLink/quickLink.d2l?ou=#{course_id}&type=dropbox&id=#{id}"
    when :quiz
      "#{base_url}/common/dialogs/quickLink/quickLink.d2l?ou=#{course_id}&type=quiz&id=#{id}"
    when :discussion_topic
      "#{base_url}/lms/discussions/admin/forum_topics_list.d2l?ou=#{course_id}"
    when :discussion_thread
      # Most direct way to a thread in the portal
      "#{base_url}/lms/discussions/admin/forum_topics_list.d2l?ou=#{course_id}&tid=#{id}"
    else
      "#{base_url}/home/#{course_id}"
    end
  end

  def ensure_array(data)
    if data.is_a?(Array)
      data
    elsif data.is_a?(Hash)
      data['Objects'] || data['Items'] || []
    else
      []
    end
  end

  private

  def read_cache(path)
    cache = ApiCache.active.find_by(path: path)
    return nil unless cache
    JSON.parse(cache.data)
  rescue => e
    puts "Cache read error: #{e.message}"
    nil
  end

  def write_cache(path, data)
    return unless data.is_a?(Hash) || data.is_a?(Array)
    # Don't cache errors
    return if data.is_a?(Hash) && (data.key?('Errors') || data.key?('ErrorMessage'))

    cache = ApiCache.find_or_initialize_by(path: path)
    cache.data = data.to_json
    cache.is_archived = false # Ensure it's active when updated
    cache.save!
  rescue => e
    puts "Cache write error: #{e.message}"
  end

  def archive_cache(path)
    ApiCache.where(path: path).update_all(is_archived: true, updated_at: Time.now)
  rescue => e
    puts "Cache archive error: #{e.message}"
  end

  def do_get(path, force_refresh: false)
    # Perform a single database lookup for both data and freshness
    # We allow reading from archived data if that's all we have and we aren't refreshing,
    # but prefer active records.
    cache_record = ApiCache.active.find_by(path: path) || ApiCache.find_by(path: path)
    cached_data = nil
    begin
      cached_data = JSON.parse(cache_record.data) if cache_record
    rescue => e
      puts "Cache parse error for #{path}: #{e.message}"
    end
    
    # 10 minute freshness window - only considered fresh if NOT archived
    is_fresh = cache_record && !cache_record.is_archived && (Time.now - cache_record.updated_at < 600)

    # Return immediately if fresh or if we have data but aren't logged in (no choice)
    if !force_refresh && cached_data && (is_fresh || !authenticated?)
      return cached_data
    end

    # If we have data but it's stale, return it now but refresh in background
    if !force_refresh && cached_data && authenticated?
      # Ensure background threads have their own DB connection
      Thread.new do
        ActiveRecord::Base.connection_pool.with_connection do
          fetch_and_cache(path)
        end
      end
      return cached_data
    end

    # No cache or forced refresh, fetch synchronously
    fetch_and_cache(path)
  end

  def dismiss_news_item(course_id, news_id)
    # Matches D2L LE API for dismissing a news item
    do_post("/d2l/api/le/#{@api_version}/#{course_id}/news/#{news_id}/dismiss", {})
  end

  def mark_notification_read(notification_id)
    # Common LP API pattern for marking notification as read
    # This varies significantly by D2L version, but we'll try the standard LP path
    do_post("/d2l/api/lp/#{@api_version}/notifications/#{notification_id}/read", {})
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
      if response.code.start_with?('2')
        puts "[Brilliant API] POST Success: #{path}"
        true
      elsif response.code == '401' || response.code == '403'
        puts "[!] AUTH ERROR #{response.code} (POST): Cookie/Token likely expired."
        raise AuthenticationError.new("Brightspace session expired", response.code.to_i)
        false
      else
        puts "[Brilliant API] POST Error #{response.code}: #{path}"
        false
      end
    rescue => e
      puts "[Brilliant API] POST Exception: #{e.message}"
      false
    end
  end

  def fetch_and_cache(path)
    return nil unless authenticated?
    return read_cache(path) if @degraded_mode
    
    # Optional: silence noisy expected 404s for discussions
    is_notoriously_noisy = path.include?('/discussions/') && path.include?('/threads/')

    puts "[Brilliant API] Fetching: #{path}" unless is_notoriously_noisy

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
      response = http.request(request)
      if response.code == '200'
        data = JSON.parse(response.body)
        write_cache(path, data)
        data
      elsif response.code == '401'
        puts "[!] AUTH ERROR 401: Token expired. Session is invalid."
        @degraded_mode = true
        read_cache(path)
      elsif response.code == '403'
        # Distinguish between global 403 (session dead) and resource 403 (archive/restricted)
        # If we hit 403 on a specific course resource, we shouldn't kill the whole app's ability to sync.
        is_global = ['/users/whoami', '/enrollments/myenrollments/'].any? { |p| path.include?(p) }
        if is_global
          puts "[!] AUTH ERROR 403: Forbidden on global resource. Entering Degraded Mode."
          @degraded_mode = true
        else
          puts "[!] ACCESS FORBIDDEN 403: #{path}. Course might be archived or restricted."
        end
        read_cache(path)
      elsif response.code == '404'
        puts "[!] 404 NOT FOUND: #{path}" unless is_notoriously_noisy
        nil
      else
        puts "[!] API ERROR #{response.code}: #{path}" unless is_notoriously_noisy
        read_cache(path)
      end
    rescue => e
      puts "[!] API EXCEPTION: #{e.message}"
      read_cache(path)
    end
  end
end
