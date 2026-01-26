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
          end
          
          # Notify user about skipped items (manual overrides protected)
          if skipped_items.any?
            create_system_notification({
              id: "sync_protection_#{Time.now.to_i}",
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

  def sync_notifications(courses, user, full_sync: false)
    puts "[Brilliant API] Syncing notifications to DB..."
    
    last_sync_key = "last_notification_sync_at"
    last_sync_time = full_sync ? nil : UserPreference.get(last_sync_key)
    
    # Sync courses to normalized table
    ActiveRecord::Base.transaction do
      courses.each do |c|
        next if c.nil? || c['OrgUnit'].nil?

        new_name = c['OrgUnit']['Name']
        next if new_name.nil? || new_name.empty?

        course = Course.find_or_initialize_by(org_unit_id: c['OrgUnit']['Id'].to_s)
        course.name = new_name if new_name.present? && !new_name.match?(/^\d+$/)
        course.code = c['OrgUnit']['Code']
        course.is_pinned = !c['PinDate'].nil?
        course.last_accessed_at = (Time.parse(c.dig('Access', 'LastAccessed')) rescue nil)
        course.semester = extract_semester_from_name(course.name)
        
        img_url = c.dig('OrgUnit', 'ImageUrl') || c.dig('OrgUnit', 'Image', 'ViewUrl') || c.dig('OrgUnit', 'Image', 'DisplayUrl')
        if img_url && !img_url.empty?
          img_url = "https://#{@host}#{img_url}" if img_url.start_with?("/")
          course.banner_url = img_url
        end
        
        course.save!
      end
    end
    archive_cache("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/")

    feed_path = "/d2l/api/lp/#{@api_version}/feed/"
    feed_path += "?since=#{URI.encode_www_form_component(last_sync_time)}" if last_sync_time
    feed_items = get_unified_feed(courses, since: last_sync_time)
    ActiveRecord::Base.transaction do
      feed_items.each do |item|
        upsert_notification(item)
      end
    end
    archive_cache(feed_path)

    courses.take(full_sync ? courses.size : 15).each do |c|
      next if c.nil? || c['OrgUnit'].nil?
      course_id = c['OrgUnit']['Id']
      next if course_id.nil?
      
      news_path = "/d2l/api/le/#{@api_version}/#{course_id}/news/"
      news_path += "?since=#{URI.encode_www_form_component(last_sync_time)}" if last_sync_time
      news_data = get_news(course_id, since: last_sync_time)
      items = ensure_array(news_data)
      
      ActiveRecord::Base.transaction do
        items.each do |item|
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

        items.each do |item|
          if item['Attachments'] && !item['Attachments'].empty?
            item['Attachments'].each do |att|
              persist_attachment(course_id, "news/#{item['Id']}/attachments/#{att['FileId']}", att['FileId'], att['FileName'])
            end
          end
        end
        archive_cache(news_path)

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

    content_items = get_content_notifications(courses, since: last_sync_time)
    ActiveRecord::Base.transaction do
      content_items.each do |item|
        upsert_notification(item)
      end
    end

    grade_items = get_recent_grades_notifications(courses)
    ActiveRecord::Base.transaction do
      grade_items.each do |item|
        upsert_notification(item)
      end
    end

    disc_items = get_discussion_notifications(courses, user['Identifier'])
    ActiveRecord::Base.transaction do
      disc_items.each do |item|
        upsert_notification(item)
      end
    end

    sync_upcoming_assignment_notifications(courses)
    
    # IMPORTANT: Prevent advancing sync timestamp if authentication failed during this run
    if !@degraded_mode
      UserPreference.set(last_sync_key, Time.now.utc.iso8601)
    end

    puts "[Brilliant API] Notification sync complete."
  end

  def sync_upcoming_assignment_notifications(courses)
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
    n = Notification.find_or_initialize_by(external_id: data[:id].to_s, course_id: data[:course_id].to_s)
    
    new_title = data[:title]
    new_body = data[:body]

    n.notification_type = data[:type]
    n.title = new_title if new_title.present?
    n.body = html_to_markdown(new_body) if new_body.present?

    raw_date = data[:date]
    begin
      if raw_date
        parsed_date = raw_date.is_a?(Time) ? raw_date : Time.parse(raw_date.to_s)
        if n.new_record? || !n.date || (parsed_date - n.date).abs > 60
          n.date = parsed_date
        end
      else
        n.date ||= Time.now
      end
    rescue => e
      n.date ||= Time.now
    end
    
    n.course_name = data[:course_name]
    
    if (n.semester.nil? || n.semester.to_s.empty?) && n.course_name
      n.semester = extract_semester_from_name(n.course_name)
    end
    n.semester = data[:semester] if data[:semester]

    n.attachments = data[:attachments] if data[:attachments]
    n.urgency = data[:urgency]
    n.is_personal = data[:is_personal]
    n.url = data[:url]
    n.save!
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
    data = do_get("/d2l/api/lp/#{@api_version}/users/whoami")
    if data
      @user_display_name = data['DisplayName']
      # Store identity in UserPreference for Polyglot Identity
      UserPreference.set('brightspace_uid', data['UniqueIdentifier'])
      UserPreference.set('brightspace_user_id', data['Identifier'])
      UserPreference.set('last_login_at', Time.now)
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
        access_time = raw_access ? Time.parse(raw_access) : Time.at(0)
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
    modules = toc.is_a?(Hash) ? (toc['Modules'] || []) : toc
    return unless modules.is_a?(Array)
    
    ActiveRecord::Base.transaction do
      modules.each_with_index do |mod, index|
        m = ContentModule.find_or_initialize_by(brightspace_id: mod['ModuleId'].to_s, course_id: course_id.to_s)
        m.title = mod['Title']
        # Extract HTML string from Description object if present, convert to Markdown
        m.description = html_to_markdown(mod['Description'])
        m.sort_order = index
        m.save!
        
        (mod['Topics'] || []).each_with_index do |topic, t_index|
          t_id = (topic['Identifier'] || topic['TopicId'] || topic['Id']).to_s
          item = ContentItem.find_or_initialize_by(brightspace_id: t_id, module_id: mod['ModuleId'].to_s)
          item.title = topic['Title']
          item.item_type = (topic['TypeIdentifier'] || topic['Type']).to_s
          item.url = topic['Url']
          item.is_hidden = topic['IsHidden'] || false
          item.sort_order = t_index
          item.attachments = [topic].to_json if topic.any?
          item.save!

          if item.url && (item.url.start_with?('/content/enforced/') || item.url.include?('/viewContent/'))
            Thread.new(course_id, item.brightspace_id, item.title) do |cid, tid, title|
              ActiveRecord::Base.connection_pool.with_connection { persist_attachment(cid, "content/topics/#{tid}/file", tid, title) }
            end
          end
        end
        sync_sub_modules(course_id, mod['ModuleId'].to_s, mod['Modules']) if mod['Modules']
      end
    end
  end

  def sync_sub_modules(course_id, parent_id, sub_modules)
    modules = sub_modules.is_a?(Hash) ? (sub_modules['Modules'] || []) : sub_modules
    return unless modules.is_a?(Array)
    
    modules.each_with_index do |mod, index|
      m = ContentModule.find_or_initialize_by(brightspace_id: mod['ModuleId'].to_s, course_id: course_id.to_s)
      m.title = mod['Title']
      # Extract HTML string from Description object if present, convert to Markdown
      m.description = html_to_markdown(mod['Description'])
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
        item.attachments = [topic].to_json if topic.any?
        item.save!

        if item.url && (item.url.start_with?('/content/enforced/') || item.url.include?('/viewContent/'))
          Thread.new(course_id, item.brightspace_id, item.title) do |cid, tid, title|
            ActiveRecord::Base.connection_pool.with_connection { persist_attachment(cid, "content/topics/#{tid}/file", tid, title) }
          end
        end
      end
      sync_sub_modules(course_id, mod['ModuleId'].to_s, mod['Modules']) if mod['Modules']
    end
  end

  def sync_assignments(course_id, assignments)
    items = ensure_array(assignments)
    skipped = []
    
    items.each do |a_summary|
      next if a_summary.nil?
      t_id = (a_summary['Id'] || a_summary['Identifier'] || a_summary['TopicId']).to_s
      a = get_assignment(course_id, t_id) || a_summary
      
      assignment = Assignment.find_or_initialize_by(brightspace_id: t_id, course_id: course_id.to_s)
      
      if assignment.manually_edited?
        skipped << assignment
        next
      end

      assignment.name = a['Name'] if a['Name'].present? && !a['Name'].match?(/^\d+$/)
      assignment.due_date = (Time.parse(a['DueDate']) rescue nil) if a['DueDate']
      
      # Handle instruction/description objects which often contain HTML
      desc_obj = a['CustomInstructions'] || a['Description']
      desc_text = ""
      if desc_obj.is_a?(Hash)
        raw_content = desc_obj['Html'] || desc_obj['Text'] || ""
        desc_text = html_to_markdown(raw_content)
      elsif desc_obj
        desc_text = html_to_markdown(desc_obj.to_s)
      end
      assignment.description = desc_text

      atts = (a['Attachments'] || []) + (a['LinkAttachments'] || [])
      assignment.attachments = atts.to_json if atts.any?
      assignment.is_graded = a['IsGraded'] || false
      assignment.grade_item_id = a['GradeItemId'].to_s if a['GradeItemId']
      assignment.save!
    end
    skipped
  end

  def sync_quizzes(course_id, quizzes)
    items = ensure_array(quizzes)
    skipped = []
    
    items.each do |q|
      next if q.nil?
      q_id = (q['QuizId'] || q['Id'] || q['Identifier']).to_s
      assignment = Assignment.find_or_initialize_by(brightspace_id: "quiz_#{q_id}", course_id: course_id.to_s)
      
      if assignment.manually_edited?
        skipped << assignment
        next
      end

      assignment.name = q['Name']
      assignment.assignment_type = 'quiz'
      assignment.due_date = (Time.parse(q['DueDate']) rescue nil) if q['DueDate']
      
      desc_obj = q['Description'] || q['Header']
      desc_text = ""
      if desc_obj.is_a?(Hash)
        raw_content = desc_obj['Html'] || desc_obj['Text'] || ""
        desc_text = html_to_markdown(raw_content)
      elsif desc_obj
        desc_text = html_to_markdown(desc_obj.to_s)
      end

      # Add instructions if present
      inst_obj = q['Instructions']
      if inst_obj.is_a?(Hash)
        inst_content = inst_obj['Html'] || inst_obj['Text'] || ""
        desc_text += "\n\n### Instructions\n#{html_to_markdown(inst_content)}" if inst_content.present?
      end
      
      assignment.description = desc_text
      assignment.save!
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
    ensure_array(forums).each do |f|
      forum = DiscussionForum.find_or_initialize_by(brightspace_id: f['ForumId'].to_s, course_id: course_id.to_s)
      forum.name = f['Name']
      forum.description = html_to_markdown(f['Description'])
      forum.save!
      topics = ensure_array(get_discussion_topics(course_id, f['ForumId']))
      sync_discussion_topics(course_id, f['ForumId'], topics)
    end
  end

  def sync_discussion_topics(course_id, forum_id, topics)
    topics.each_with_index do |t, index|
      topic = DiscussionTopic.find_or_initialize_by(brightspace_id: t['TopicId'].to_s, forum_id: forum_id.to_s)
      topic.course_id = course_id.to_s
      topic.name = t['Name']
      topic.description = html_to_markdown(t['Description'])
      topic.sort_order = index
      topic.save!
    end
  end

  def sync_grades(course_id, grade_values)
    ensure_array(grade_values).each do |g|
      obj_id = (g['GradeObjectIdentifier'] || g['Identifier']).to_s
      grade = Grade.find_or_initialize_by(brightspace_id: obj_id, course_id: course_id.to_s)
      grade.name = g['GradeObjectName'] if g['GradeObjectName'].present? && !g['GradeObjectName'].match?(/^\d+$/)
      grade.displayed_grade = g['DisplayedGrade']
      grade.save!
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
    cache_record = ApiCache.active.find_by(path: path) || ApiCache.find_by(path: path)
    cached_data = JSON.parse(cache_record.data) rescue nil
    
    is_fresh = cache_record && !cache_record.is_archived && (Time.now - cache_record.updated_at < 600)

    return cached_data if !force_refresh && cached_data && (is_fresh || !authenticated?)

    if !force_refresh && cached_data && authenticated?
      Thread.new { ActiveRecord::Base.connection_pool.with_connection { fetch_and_cache(path) } }
      return cached_data
    end

    fetch_and_cache(path)
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
    cache = ApiCache.active.find_by(path: path)
    JSON.parse(cache.data) rescue nil
  end

  def write_cache(path, data)
    return unless data.is_a?(Hash) || data.is_a?(Array)
    return if data.is_a?(Hash) && (data.key?('Errors') || data.key?('ErrorMessage'))
    cache = ApiCache.find_or_initialize_by(path: path)
    cache.data = data.to_json
    cache.is_archived = false
    cache.save!
  end

  def archive_cache(path)
    ApiCache.where(path: path).update_all(is_archived: true, updated_at: Time.now)
  end

  def fetch_and_cache(path)
    return nil unless authenticated?
    return read_cache(path) if @degraded_mode

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
      if response.code.start_with?('2')
        @degraded_mode = false
        @auth_notification_sent = false
        data = JSON.parse(response.body)
        write_cache(path, data)
        data
      elsif ['401', '403'].include?(response.code)
        handle_auth_failure(response.code)
        read_cache(path)
      else
        read_cache(path)
      end
    rescue
      read_cache(path)
    end
  end

  def handle_auth_failure(code)
    @degraded_mode = true
    # We no longer create a persistent Notification object for auth failure
    # because the user is notified via the global Flash/Banner in the UI.
    @auth_notification_sent = true
  end
end
