require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'
require 'time'
require 'set'

class BrilliantClient
  class AuthenticationError < StandardError; attr_reader :status_code; def initialize(msg, code); super(msg); @status_code = code; end; end
  
  attr_accessor :token, :cookie_string, :host, :user_display_name, :sync_status, :degraded_mode, :last_auth_error

  def initialize
    data_dir = ENV['BRILLIANT_DATA_DIR']
    
    # Fallback for CLI usage to find Electron data
    if data_dir.nil?
      electron_data = File.expand_path("~/Library/Application Support/Brilliant")
      if Dir.exist?(electron_data)
        data_dir = electron_data
        puts "[Brilliant] Using Electron data directory: #{data_dir}"
      else
        data_dir = '.'
      end
    end
    
    @config_path = File.join(data_dir, 'config', 'connection.json')
    load_connection_config
    
    @api_version = "1.40"
    @sync_lock = Mutex.new
    @syncing = false
    @sync_status = { status: "idle", progress: 0, current_task: nil }
    @degraded_mode = false
    @last_auth_error = nil
    @auth_notification_sent = false
    @in_flight_requests = {}
    @pending_tasks = {}

    # Initialize Services
    @notification_service = Brilliant::Sync::NotificationService.new(self)
    @content_service = Brilliant::Sync::ContentService.new(self)
    @assignment_service = Brilliant::Sync::AssignmentService.new(self)
    @discussion_service = Brilliant::Sync::DiscussionService.new(self)
    @grade_service = Brilliant::Sync::GradeService.new(self)
    @psy220_scraper = Brilliant::Sync::Psy220ScraperService.new(self)

    # Background Worker
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
    
    # Background Periodic Sync (runs every 30 minutes to keep data fresh)
    Thread.new do
      loop do
        sleep 1800 # 30 minutes
        begin
          if authenticated? && !@syncing
            puts "[Periodic Sync] Starting background sync..."
            sync_all_courses_proactively
          end
        rescue => e
          puts "[Periodic Sync] Failed: #{e.message}"
        end
      end
    end
    
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
    # Robust host normalization
    @host = host.to_s.gsub(/https?:\/\//, '').split('/').first.strip
    
    # Robust cookie normalization: remove 'Cookie:' prefix, strip whitespace, 
    # and handle multiple lines if the user copied from a header block.
    # Also handles semicolon-separated pairs more cleanly.
    cleaned_cookies = cookies.to_s
      .gsub(/^Cookie:\s*/i, '')     # Remove 'Cookie: ' prefix
      .strip                        # Remove leading/trailing whitespace
      .gsub(/[\r\n]+/, '; ')        # Convert newlines to semicolons
      .gsub(/;\s*;/, ';')           # Remove empty pairs
      .gsub(/\s+/, ' ')             # Normalize spaces
      .strip
    
    @cookie_string = cleaned_cookies
    
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

          # Reset full sync flag if set
          UserPreference.set('force_full_sync', 'false') if full_sync

          courses.each do |c|
            begin
              course_id = c['OrgUnit']['Id']
              course_name = c['OrgUnit']['Name']
              puts "[Sync] Processing Course: #{course_name} (ID: #{course_id})"
              
              # Simple truncation helper for status display
              short_name = course_name.length > 10 ? course_name[0...9] + "…" : course_name

              @sync_status[:current_task] = "#{short_name} - Syncing Core Content..."
              
              # Sync Core (Force refresh during proactive sync to ensure accuracy)
              toc_path = "/d2l/api/le/#{@api_version}/#{course_id}/content/toc"
              toc = get_toc(course_id, force_refresh: true)
              puts "[Sync] TOC for #{course_id} fetched: #{toc.is_a?(Hash) ? toc.keys : toc.class}"
              sync_course_content(course_id, toc) if toc
              archive_cache(toc_path) if toc
              
              sleep 0.1
              @sync_status[:current_task] = "#{short_name} - Syncing Assignments..."
              assign_path = "/d2l/api/le/#{@api_version}/#{course_id}/dropbox/folders/"
              assignments = get_assignments(course_id, force_refresh: true)
              puts "[Sync] Assignments for #{course_id} fetched: #{assignments.is_a?(Array) ? assignments.size : assignments.class}"
              skipped_items += sync_assignments(course_id, assignments) if assignments
              archive_cache(assign_path) if assignments

              sleep 0.1
              @sync_status[:current_task] = "#{short_name} - Syncing Quizzes..."
              quiz_path = "/d2l/api/le/#{@api_version}/#{course_id}/quizzes/"
              quizzes = get_quizzes(course_id, force_refresh: true)
              skipped_items += sync_quizzes(course_id, quizzes) if quizzes
              archive_cache(quiz_path) if quizzes
              
              sleep 0.1
              
              @sync_status[:current_task] = "#{short_name} - Syncing Discussions..."
              # Sync Discussions
              disc_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/"
              forums = get_discussions(course_id, force_refresh: true) || []
              sync_discussions(course_id, forums) if forums.any?
              archive_cache(disc_path) if forums.any?
              
              sleep 0.1
              
              @sync_status[:current_task] = "#{short_name} - Syncing Grades..."
              # Sync Grades
              grades_path = "/d2l/api/le/#{@api_version}/#{course_id}/grades/values/myGradeValues/"
              grades_raw = get_grades(course_id, force_refresh: true)
              sync_grades(course_id, grades_raw) if grades_raw.is_a?(Array)
              archive_cache(grades_path) if grades_raw.is_a?(Array)

              current_step += 1
              @sync_status[:progress] = ((current_step.to_f / total_steps) * 100).to_i

              # Publish course-wide update event
              Brilliant::EventBus.publish(:course_overview_updated, { course_id: course_id })
              Brilliant::EventBus.publish(:assignments_updated, { course_id: course_id })
              Brilliant::EventBus.publish(:grades_updated, { course_id: course_id })
            rescue => e
              puts "[Sync] Failed to process course #{course_id}: #{e.message}"
              puts e.backtrace.first(5).join("\n")
              next
            end
          end
          
          @sync_status[:current_task] = "Syncing Notifications..."
          sync_notifications(courses, user, full_sync: full_sync)
          
          current_step += 1
          @sync_status[:progress] = ((current_step.to_f / total_steps) * 100).to_i

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
    @notification_service.sync(courses, user, full_sync: full_sync, limit: limit)
  end

  def sync_course_content(course_id, toc)
    @content_service.sync(course_id, toc)
  end

  def sync_assignments(course_id, assignments)
    @assignment_service.sync_dropbox(course_id, assignments)
  end

  def sync_quizzes(course_id, quizzes)
    @assignment_service.sync_quizzes(course_id, quizzes)
  end

  def sync_discussions(course_id, forums)
    @discussion_service.sync(course_id, forums)
  end

  def sync_grades(course_id, grades_raw)
    @grade_service.sync(course_id, grades_raw)
    
    # Special handling for PSY-220 which has hidden/ungraded items in the API
    if course_id.to_s == '446900'
      @psy220_scraper.sync(course_id)
    end
  end

  def get_global_alerts(since: nil)
    path = "/d2l/api/lp/#{@api_version}/alerts/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    data = do_get(path, force_refresh: true)
    ensure_array(data)
  end

  def map_alerts_to_notifications(alerts, courses)
    course_map = courses.each_with_object({}) { |c, h| h[c['OrgUnit']['Id'].to_s] = c['OrgUnit']['Name'] }
    alerts.map do |alert|
      course_id = alert['OrgUnitId'].to_s
      type = alert['Type']
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

  def sync_upcoming_assignment_notifications(courses)
    upcoming_limit = Time.current + 7.days
    ActiveRecord::Base.connection_pool.with_connection do
      Assignment.where("due_date > ? AND due_date <= ?", Time.current, upcoming_limit).each do |a|
        next if a.nil?
        begin
          course_obj = Course.find_by(org_unit_id: a.course_id.to_s)
          course_name = course_obj&.name || "Unknown Course"
          type_label = a.assignment_type == 'quiz' ? 'Quiz' : 'Assignment'
          url = a.assignment_type == 'quiz' ? "/course/#{a.course_id}/quizzes/#{a.brightspace_id.sub('quiz_', '')}" : "/course/#{a.course_id}/assignments/#{a.brightspace_id}"
          url += '?edit=true' if a.synthetic
          body_text = "This #{type_label.downcase} is due on #{a.due_date.strftime('%A, %b %d at %I:%M %p')}."
          body_text += " [Open Link](#{a.external_url})" if a.synthetic && a.external_url.present?
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
    
    # Ensure Time.zone is available for parsing
    Time.zone ||= 'UTC'

    # Use a slightly earlier 'since' to account for D2L indexing lag and second-level precision issues
    adjusted_since = if since
      (Time.zone.parse(since) - 2.minutes).iso8601 rescue since
    else
      # Fallback to last 7 days if no since provided (instead of 24h) to ensure we catch recent but not immediate updates
      (Time.current - 7.days).iso8601
    end

    courses.each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      path = "/d2l/api/le/#{@api_version}/#{course_id}/content/updates"
      path += "?since=#{URI.encode_www_form_component(adjusted_since)}" if adjusted_since
      
      puts "[Sync] Checking content updates for #{c['OrgUnit']['Name']} since #{adjusted_since}..."
      data = do_get(path, force_refresh: true)
      
      # Process API items
      api_items = ensure_array(data)
      puts "[Sync] Found #{api_items.size} content updates for #{c['OrgUnit']['Name']} via API" if api_items.any?
      
      # Track seen IDs to avoid duplicates between API and DB
      seen_ids = Set.new
      
      api_items.each do |item|
        item_id = (item['ContentObjectIdentifier'] || item['Identifier'] || item['Id'] || item['TopicId']).to_s
        seen_ids << item_id if item_id.present?
        
        title = item['Title']
        if (title.nil? || title.empty?) && !item_id.empty?
          db_item = ContentItem.joins(:content_module).find_by(brightspace_id: item_id, content_modules: { course_id: course_id.to_s })
          db_item ||= ContentItem.find_by(brightspace_id: item_id)
          db_mod = ContentModule.find_by(brightspace_id: item_id, course_id: course_id.to_s) if db_item.nil?
          title = (db_item || db_mod)&.title
        end
        
        all_updates << build_content_notification(c, item_id, title, item['LastModifiedDate'] || item['CreatedDate'])
      end
      
      # 2. Fallback/Augment: Check local DB for things recently updated
      # This is crucial because TOC sync (sync_course_content) just ran and populated the DB.
      # The /content/updates API can sometimes lag or miss items.
      begin
        # Use a tighter window for DB fallback if this is a first sync to avoid flooding
        db_since = since ? (Time.zone.parse(since) - 2.minutes) : (Time.current - 12.hours)
        
        # Check ContentItems
        ContentItem.joins(:content_module)
                    .where(content_modules: { course_id: course_id.to_s })
                    .where("content_items.updated_at > ?", db_since)
                    .find_each do |db_item|
          next if seen_ids.include?(db_item.brightspace_id.to_s)
          
          puts "[Sync] Found content item update for #{c['OrgUnit']['Name']} via DB: #{db_item.title}"
          all_updates << build_content_notification(c, db_item.brightspace_id, db_item.title, db_item.updated_at)
          seen_ids << db_item.brightspace_id.to_s
        end

        # Check ContentModules
        ContentModule.where(course_id: course_id.to_s)
                      .where("updated_at > ?", db_since)
                      .find_each do |db_mod|
          next if seen_ids.include?(db_mod.brightspace_id.to_s)
          
          puts "[Sync] Found content module update for #{c['OrgUnit']['Name']} via DB: #{db_mod.title}"
          all_updates << build_content_notification(c, db_mod.brightspace_id, db_mod.title, db_mod.updated_at)
          seen_ids << db_mod.brightspace_id.to_s
        end
      rescue => e
        puts "[Sync] Local DB fallback failed for #{c['OrgUnit']['Name']}: #{e.message}"
      end

      archive_cache(path)
    end
    all_updates
  end

  def authenticated?
    # Must have either a token or a non-empty cookie string
    return true if !@token.nil?
    return false if @cookie_string.nil? || @cookie_string.strip.empty?
    
    # Basic validation: Brightspace session cookies usually contain 'd2lSessionVal'
    # or at least a few key-value pairs.
    @cookie_string.include?('=') && @cookie_string.length > 20
  end

  # --- End Public API ---

  def build_content_notification(course_data, item_id, title, date)
    course_id = course_data['OrgUnit']['Id']
    display_title = title.present? ? title : "New Content Item"
    course_code = extract_course_code(course_data['OrgUnit']['Name'])
    
    final_title = if course_code && !display_title.include?(course_code)
      "#{course_code}: #{display_title}"
    else
      display_title
    end

    # Use the date provided, or fallback to now
    parsed_date = date.is_a?(String) ? (Time.zone.parse(date) rescue Time.current) : (date || Time.current)

    {
      id: "content_#{course_id}_#{item_id.present? ? item_id : SecureRandom.hex(4)}",
      type: 'Content',
      title: "Content Updated: #{final_title}",
      body: "New or updated content in #{course_data['OrgUnit']['Name']}: #{display_title}",
      date: parsed_date,
      last_modified: (date if date.is_a?(String) && date.include?('T')),
      course_id: course_id,
      course_name: course_data['OrgUnit']['Name'],
      urgency: 1,
      is_personal: false,
      url: "/course/#{course_id}"
    }
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

  def extract_course_code(full_name)
    return nil unless full_name
    # Matches patterns like "SWO 370", "PSY-220", "MAT101"
    match = full_name.match(/([A-Z]{2,4}\s*[-]?\s*\d{3,4})/i)
    match[1].strip if match
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
      ActiveRecord::Base.connection_pool.with_connection do
        pref = UserPreference.current
        pref.update(brightspace_uid: data['UniqueIdentifier'], brightspace_user_id: data['Identifier'], last_login_at: Time.current)
      end
    end
    data
  end

  def get_enrollments(force_refresh: false)
    response = do_get("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/", force_refresh: force_refresh)
    return [] unless response
    items = ensure_array(response)
    items.select do |i| 
      (i.dig('OrgUnit', 'Type', 'Code') == 'Course Offering' || i.dig('OrgUnit', 'Type', 'Name') == 'Course Offering')
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

  def get_toc(org_unit_id, force_refresh: false)
    data = do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/content/toc", force_refresh: force_refresh)
    if data.is_a?(Array)
      { 'Modules' => data }
    elsif data.is_a?(Hash) && data['Modules']
      data
    else
      { 'Modules' => [] }
    end
  end

  def get_assignments(org_unit_id, force_refresh: false)
    get_all_pages("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/", force_refresh: force_refresh)
  end

  def get_quizzes(org_unit_id, force_refresh: false)
    get_all_pages("/d2l/api/le/#{@api_version}/#{org_unit_id}/quizzes/", force_refresh: force_refresh)
  end

  def get_assignment(org_unit_id, assignment_id, force_refresh: false)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/#{assignment_id}", force_refresh: force_refresh)
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
    get_all_pages("/d2l/api/le/#{@api_version}/#{org_unit_id}/grades/values/myGradeValues/", force_refresh: force_refresh)
  end

  def get_grade_definitions(org_unit_id, force_refresh: false)
    get_all_pages("/d2l/api/le/#{@api_version}/#{org_unit_id}/grades/", force_refresh: force_refresh)
  end

  def get_all_pages(path, force_refresh: false)
    all_items = []
    current_path = path
    
    loop do
      data = do_get(current_path, force_refresh: force_refresh)
      break unless data
      
      items = ensure_array(data)
      all_items.concat(items)
      
      # Handle PagingInfo
      # D2L uses PagingInfo { HasMoreItems: true, Bookmark: "..." }
      paging = data.is_a?(Hash) ? data['PagingInfo'] : nil
      if paging && paging['HasMoreItems'] && paging['Bookmark']
        # Add bookmark to query string
        uri = URI.parse(path)
        params = URI.decode_www_form(uri.query || "") << ["bookmark", paging['Bookmark']]
        uri.query = URI.encode_www_form(params)
        current_path = uri.to_s
        # Force fresh fetch for subsequent pages if the first one was fresh
        force_refresh = true if force_refresh
      else
        break
      end
      
      # Safety break to avoid infinite loops
      break if all_items.size > 2000
    end
    
    all_items
  end

  def get_discussions(org_unit_id, force_refresh: false)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/", force_refresh: force_refresh)
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

  def get_unified_feed(courses = [], since: nil, force_refresh: false)
    course_map = courses.each_with_object({}) { |c, h| h[c['OrgUnit']['Id'].to_s] = c['OrgUnit']['Name'] }
    path = "/d2l/api/lp/#{@api_version}/feed/"
    path += "?since=#{URI.encode_www_form_component(since)}" if since
    feed_data = do_get(path, force_refresh: force_refresh)
    feed = ensure_array(feed_data)
    feed.map do |item|
      cid = item.dig('Metadata', 'ApiViewUrl')&.match(/\/(\d+)\/news/)&.to_a&.at(1)
      news_id = item.dig('Metadata', 'Identifier')
      {
        id: cid ? "news_#{cid}_#{news_id}" : news_id,
        type: 'News',
        title: item.dig('Metadata', 'Title'),
        body: item.dig('Metadata', 'Summary', 'Text'),
        date: item.dig('Metadata', 'Date') || item.dig('Resource', 'LastModifiedDate') || item.dig('Resource', 'CreatedDate'),
        last_modified: item.dig('Resource', 'LastModifiedDate'),
        created_at: item.dig('Resource', 'CreatedDate'),
        course_id: cid,
        course_name: cid ? course_map[cid.to_s] : "General",
        urgency: 1,
        is_personal: false,
        url: item.dig('Metadata', 'WebViewUrl') ? "https://#{@host}#{item.dig('Metadata', 'WebViewUrl')}" : nil
      }
    end
  end

  def enqueue_attachment_task(course_id, api_path, file_id, file_name)
    @task_queue << proc { persist_attachment(course_id, api_path, file_id, file_name) }
  end

  def html_to_markdown(html)
    Brilliant::TextProcessor.html_to_markdown(html)
  end

  def upsert_notification(data)
    @notification_service.upsert_notification_batch([data], publish_event_flag: true)
  end

  def create_system_notification(data)
    # Ensure system notifications have required fields for the schema
    data[:course_id] ||= "system"
    data[:course_name] ||= "System"
    data[:type] ||= "System"
    data[:date] ||= Time.current.iso8601
    upsert_notification(data)
  end

  def upsert_notification_batch(items, publish_event: false)
    @notification_service.upsert_notification_batch(items, publish_event_flag: publish_event)
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

  private

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
      request['User-Agent'] = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"

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
        puts "[API Error] #{response.code} for #{path}: #{response.body[0..200]}"
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
    puts "[Auth] Authentication failure (Code: #{code}) for path: #{path}"
    
    # Verify if the session is truly dead using the "whoami" probe
    # This avoids entering degraded mode due to transient errors or restricted resource access
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

      # Use a very short timeout for the probe
      http = Net::HTTP.new(uri.host, uri.port)
      http.use_ssl = true
      http.read_timeout = 5
      http.open_timeout = 5
      
      probe_res = http.request(request)
      if probe_res.code.start_with?('2')
        puts "[Auth] #{code} suppressed for #{path} - Session still valid via whoami probe."
        return
      end
    rescue => e
      puts "[Auth] Probe failed during #{code} handling: #{e.message}"
    end

    @degraded_mode = true
    @auth_notification_sent = true
    Brilliant::EventBus.publish(:authentication_failure, { code: code, host: @host })
  end
end
