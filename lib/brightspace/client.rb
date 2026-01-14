require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'
require 'time'

class BrightspaceClient
  attr_accessor :token, :cookie_string, :host, :user_display_name, :sync_status

  def initialize
    @config_path = 'config/connection.json'
    load_connection_config
    
    @api_version = "1.40"
    @sync_lock = Mutex.new
    @syncing = false
    @sync_status = { status: "idle", progress: 0, current_task: nil }
    
    # Legacy/Fallback: Try loading from cookies.txt
    load_cookies_from_file if !authenticated? && File.exist?('cookies.txt')
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
    @host = host
    @cookie_string = cookies.sub(/^Cookie:\s*/i, '')
    
    FileUtils.mkdir_p('config')
    File.write(@config_path, { host: @host, cookies: @cookie_string }.to_json)
  end

  def sync_all_courses_proactively
    return if @syncing
    
    Thread.new do
      ActiveRecord::Base.connection_pool.with_connection do
        @sync_lock.synchronize { @syncing = true }
        @sync_status = { status: "syncing", progress: 0, current_task: "Starting proactive sync..." }
        
        begin
          courses = get_enrollments || []
          user = get_who_am_i
          
          total_steps = 1 + courses.size # Notifications + each course
          current_step = 0

          # Check for a "full sync" request or just differential
          full_sync = ActiveRecord::Base.connection_pool.with_connection { 
            UserPreference.get('force_full_sync') == 'true'
          }

          @sync_status[:current_task] = "Syncing Notifications..."
          sync_notifications(courses, user, full_sync: full_sync)
          
          # Reset full sync flag if set
          ActiveRecord::Base.connection_pool.with_connection { UserPreference.set('force_full_sync', 'false') } if full_sync

          current_step += 1
          @sync_status[:progress] = ((current_step.to_f / total_steps) * 100).to_i

          courses.each do |c|
            course_id = c['OrgUnit']['Id']
            course_name = c['OrgUnit']['Name']
            @sync_status[:current_task] = "Syncing #{course_name}..."
            
            # Sync Core
            toc = get_toc(course_id)
            sync_course_content(course_id, toc) if toc
            
            sleep 0.1
            assignments = get_assignments(course_id)
            sync_assignments(course_id, assignments) if assignments
            
            sleep 0.1
            
            # Sync Discussions
            forums = get_discussions(course_id) || []
            sync_discussions(course_id, forums) if forums.any?
            
            sleep 0.1
            
            # Sync Grades
            grades_raw = get_grades(course_id)
            sync_grades(course_id, grades_raw) if grades_raw.is_a?(Array)

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
    puts "[Brightspace API] Syncing notifications to DB..."
    
    last_sync_key = "last_notification_sync_at"
    last_sync_time = full_sync ? nil : UserPreference.get(last_sync_key)
    since_param = last_sync_time ? "?since=#{URI.encode_www_form_component(last_sync_time)}" : ""
    
    # Sync courses to normalized table
    ActiveRecord::Base.transaction do
      courses.each do |c|
        course = Course.find_or_initialize_by(org_unit_id: c['OrgUnit']['Id'].to_s)
        course.name = c['OrgUnit']['Name']
        course.code = c['OrgUnit']['Code']
        course.is_pinned = !c['PinDate'].nil?
        course.last_accessed_at = Time.parse(c.dig('Access', 'LastAccessed')) rescue nil
        course.semester = extract_semester_from_name(course.name)
        course.save!
      end
    end

    # Get unified feed first (broad coverage)
    feed_items = get_unified_feed(courses, since: last_sync_time)
    ActiveRecord::Base.transaction do
      feed_items.each do |item|
        upsert_notification(item)
      end
    end

    # Get per-course News/Announcements explicitly
    courses.take(full_sync ? courses.size : 15).each do |c|
      course_id = c['OrgUnit']['Id']
      
      # News/Announcements
      news_data = get_news(course_id, since: last_sync_time)
      items = news_data.is_a?(Array) ? news_data : (news_data['Items'] || [])
      
      ActiveRecord::Base.transaction do
        items.each do |item|
          upsert_notification({
            id: "news_#{course_id}_#{item['Id']}",
            type: 'News',
            title: item['Title'],
            body: item.dig('Summary', 'Text') || item.dig('Body', 'Text'),
            date: item['StartDate'] || item['LastModifiedDate'] || item['CreatedDate'],
            course_id: course_id,
            course_name: c['OrgUnit']['Name'],
            urgency: 1,
            is_personal: false,
            url: "/course/#{course_id}/announcements"
          })
        end

        # Course Overview (requested specifically by user)
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
    
    UserPreference.set(last_sync_key, Time.now.utc.iso8601)

    puts "[Brightspace API] Notification sync complete."
  end

  def get_content_notifications(courses, since: nil)
    all_updates = []
    # Only check recent courses to avoid long sync
    courses.take(20).each do |c|
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
    n.notification_type = data[:type]
    n.title = data[:title]
    n.body = data[:body]

    # Improved date handling: handle strings, Time objects, and nil
    raw_date = data[:date]
    begin
      if raw_date
        parsed_date = raw_date.is_a?(Time) ? raw_date : Time.parse(raw_date.to_s)
        
        # Only update date if it's a new record OR if the date is significantly different
        # (to avoid constant timestamp shifting for items without stable API dates)
        if n.new_record? || !n.date || (parsed_date - n.date).abs > 60
          n.date = parsed_date
        end
      else
        n.date ||= Time.now
      end
    rescue => e
      puts "[Brightspace API] Date parse error for #{data[:id]}: #{e.message} (Raw: #{raw_date})"
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

  def get_who_am_i
    data = do_get("/d2l/api/lp/#{@api_version}/users/whoami")
    @user_display_name = data['DisplayName'] if data
    data
  end

  def get_enrollments
    response = do_get("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/")
    return [] unless response
    
    items = response['Items'] || response
    items.select do |i| 
      (i.dig('OrgUnit', 'Type', 'Code') == 'Course Offering' || 
       i.dig('OrgUnit', 'Type', 'Name') == 'Course Offering')
    end.sort_by do |i|
      pin_score = i['PinDate'] ? 0 : 1
      access_date = i.dig('Access', 'LastAccessed') || "0000-00-00"
      [pin_score, -Time.parse(access_date).to_i]
    end
  end

  def get_toc(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/content/toc")
  end

  def get_assignments(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/dropbox/folders/")
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

  def get_grades(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/grades/values/myGradeValues/")
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
      topics = topics_data.is_a?(Hash) ? (topics_data['Items'] || []) : (topics_data || [])
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
      puts "[Brightspace API] Threads 404 or empty for topic #{topic_id}. Attempting synthesis from posts..."
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
    items.sort_by { |i| i['StartDate'] || "" }.reverse.take(10)
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
    
    feed = do_get(path) || []
    
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
    return unless toc.is_a?(Array)
    
    toc.each_with_index do |mod, index|
      m = ContentModule.find_or_initialize_by(brightspace_id: mod['ModuleId'].to_s, course_id: course_id.to_s)
      m.title = mod['Title']
      m.description = mod.dig('Description', 'Text')
      m.sort_order = index
      m.save!
      
      # Sync topics/items
      (mod['Topics'] || []).each_with_index do |topic, t_index|
        item = ContentItem.find_or_initialize_by(brightspace_id: topic['Identifier'].to_s, module_id: mod['ModuleId'].to_s)
        item.title = topic['Title']
        item.item_type = topic['TypeIdentifier'] || topic['Type'] || 'Topic'
        item.url = topic['Url']
        item.is_hidden = topic['IsHidden'] || false
        item.sort_order = t_index
        item.save!
      end
      
      # Recursively sync sub-modules
      sync_sub_modules(course_id, mod['ModuleId'].to_s, mod['Modules']) if mod['Modules']
    end
  end

  def sync_sub_modules(course_id, parent_id, sub_modules)
    return unless sub_modules.is_a?(Array)
    
    sub_modules.each_with_index do |mod, index|
      m = ContentModule.find_or_initialize_by(brightspace_id: mod['ModuleId'].to_s, course_id: course_id.to_s)
      m.title = mod['Title']
      m.description = mod.dig('Description', 'Text')
      m.sort_order = index
      m.parent_id = parent_id
      m.save!
      
      (mod['Topics'] || []).each_with_index do |topic, t_index|
        item = ContentItem.find_or_initialize_by(brightspace_id: topic['Identifier'].to_s, module_id: mod['ModuleId'].to_s)
        item.title = topic['Title']
        item.item_type = topic['TypeIdentifier'] || topic['Type'] || 'Topic'
        item.url = topic['Url']
        item.is_hidden = topic['IsHidden'] || false
        item.sort_order = t_index
        item.save!
      end
      
      sync_sub_modules(course_id, mod['ModuleId'].to_s, mod['Modules']) if mod['Modules']
    end
  end

  def sync_assignments(course_id, assignments)
    items = assignments.is_a?(Hash) ? (assignments['Items'] || []) : assignments
    return unless items.is_a?(Array)
    
    items.each do |a|
      assignment = Assignment.find_or_initialize_by(brightspace_id: a['Id'].to_s, course_id: course_id.to_s)
      assignment.name = a['Name']
      assignment.due_date = Time.parse(a['DueDate']) rescue nil
      assignment.description = a.dig('CustomInstructions', 'Text') || a.dig('Description', 'Text')
      assignment.is_graded = a['IsGraded'] || false
      assignment.grade_item_id = a['GradeItemId'].to_s if a['GradeItemId']
      assignment.save!
    end
  end

  def sync_discussions(course_id, forums)
    return unless forums.is_a?(Array)
    
    forums.each do |f|
      forum = DiscussionForum.find_or_initialize_by(brightspace_id: f['ForumId'].to_s, course_id: course_id.to_s)
      forum.name = f['Name']
      forum.description = f.dig('Description', 'Text')
      forum.save!
      
      topics_data = get_discussion_topics(course_id, f['ForumId'])
      topics = topics_data.is_a?(Hash) ? (topics_data['Items'] || []) : (topics_data || [])
      sync_discussion_topics(course_id, f['ForumId'].to_s, topics) if topics
    end
  end

  def sync_discussion_topics(course_id, forum_id, topics)
    return unless topics.is_a?(Array)
    
    topics.each_with_index do |t, index|
      topic = DiscussionTopic.find_or_initialize_by(brightspace_id: t['TopicId'].to_s, forum_id: forum_id.to_s)
      topic.course_id = course_id.to_s
      topic.name = t['Name']
      topic.description = t.dig('Description', 'Html') || t.dig('Description', 'Text')
      topic.sort_order = index
      
      # D2L API can be inconsistent with count keys
      topic.thread_count = t['ThreadCount'] || t['TotalThreads'] || t['Threads'] || 0
      topic.post_count = t['PostCount'] || t['TotalPosts'] || t['Posts'] || 0
      
      lpd = t['LastPostDate'] || t['LastPost']&.fetch('DatePosted', nil)
      topic.last_post_date = Time.parse(lpd.to_s) rescue nil if lpd
      
      topic.save!
      
      # We don't sync threads here by default as it's too heavy for a broad sync,
      # but we provide the method for specific topic refreshes.
    end
  end

  def sync_topic_posts(course_id, forum_id, topic_id, posts)
    return unless posts.is_a?(Array)
    
    ActiveRecord::Base.transaction do
      posts.each do |p|
        post = DiscussionPost.find_or_initialize_by(
          brightspace_id: p['PostId'].to_s,
          topic_id: topic_id.to_s,
          thread_id: p['ThreadId'].to_s
        )
        post.parent_post_id = p['ParentPostId'].to_s if p['ParentPostId']
        post.subject = p['Subject']
        # RichText handling
        post.body = p.dig('Body', 'Html') || p.dig('Body', 'Text') || p['Body']
        post.author_name = p['PostingUserDisplayName']
        post.posted_at = Time.parse(p['DatePosted']) rescue nil
        
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
    t = DiscussionThread.find_or_initialize_by(brightspace_id: thread['ThreadId'].to_s, topic_id: topic_id.to_s)
    t.course_id = course_id.to_s
    t.subject = thread['Subject'] || thread['Title']
    t.body = thread.dig('Description', 'Text') || thread.dig('Body', 'Text')
    t.author_name = thread['PostingUserDisplayName'] || thread.dig('LastPost', 'UserDisplayName')
    t.posted_at = Time.parse(thread['DatePosted'] || thread['LastModified']) rescue nil
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
    metadata = metadata_raw.is_a?(Array) ? metadata_raw : (metadata_raw['Items'] || [])
    
    weights_map = {}
    metadata.each do |m|
      weights_map[m['Id'].to_s] = m['Weight']
    end

    ActiveRecord::Base.transaction do
      grade_values.each do |g|
        obj_id = g['GradeObjectIdentifier'].to_s
        grade = Grade.find_or_initialize_by(brightspace_id: obj_id, course_id: course_id.to_s)
        grade.name = g['GradeObjectName']
        grade.displayed_grade = g['DisplayedGrade']
        grade.numerator = g.dig('PointsNumerator')
        grade.denominator = g.dig('PointsDenominator')
        grade.weight = weights_map[obj_id] || g.dig('Weight')
        grade.grade_object_type = g['GradeObjectType']
        grade.last_modified = Time.parse(g['LastModified']) rescue nil
        grade.comments = g.dig('Comments', 'Html') || g.dig('Comments', 'Text')
        
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
      course_id = c['OrgUnit']['Id']
      grades = get_grades(course_id) || []
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
      course_id = c['OrgUnit']['Id']
      topics = get_all_topics(course_id) || []
      
      topics.each do |t|
        # Only check if unread counts exist to minimize storming
        # (Though we'll need to fetch threads to check for personal involvement)
        threads_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/#{t['ForumId']}/topics/#{t['TopicId']}/threads/"
        threads_data = do_get(threads_path)
        
        threads = threads_data.is_a?(Hash) ? (threads_data['Items'] || []) : (threads_data || [])
        
        threads.each do |th|
          next if (th['UnreadReplyCount'] || 0) == 0
          
          posts_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/#{t['ForumId']}/topics/#{t['TopicId']}/threads/#{th['ThreadId']}/posts/"
          posts_data = do_get(posts_path)
          posts = posts_data.is_a?(Hash) ? (posts_data['Items'] || []) : (posts_data || [])
          
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

  def download_file(path)
    return nil unless authenticated?

    uri = URI("https://#{@host}#{path}")
    request = Net::HTTP::Get.new(uri)
    
    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string
    end

    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true
    
    begin
      http.request(request)
    rescue => e
      puts "Download connection error for #{path}: #{e.message}"
      nil
    end
  end

  def portal_url_for(type, options = {})
    course_id = options[:course_id]
    id = options[:id]
    
    base_url = "https://#{@host}/d2l"
    
    case type
    when :course_home
      "#{base_url}/home/#{course_id}"
    when :assignment
      "#{base_url}/lms/dropbox/user/folder_submit_files.d2l?db=#{id}&ou=#{course_id}"
    when :discussion_topic
      "#{base_url}/lms/discussions/admin/forum_topics_list.d2l?ou=#{course_id}"
    when :discussion_thread
      # Most direct way to a thread in the portal
      "#{base_url}/lms/discussions/admin/forum_topics_list.d2l?ou=#{course_id}&tid=#{id}"
    else
      "#{base_url}/home/#{course_id}"
    end
  end

  private

  def read_cache(path)
    cache = ApiCache.find_by(path: path)
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
    cache.save!
  rescue => e
    puts "Cache write error: #{e.message}"
  end

  def do_get(path, force_refresh: false)
    cached_data = read_cache(path)
    cache_record = ApiCache.find_by(path: path)
    
    is_fresh = cache_record && (Time.now - cache_record.updated_at < 600) # 10 mins

    if !force_refresh && cached_data && (is_fresh || !authenticated?)
      return cached_data
    end

    if !force_refresh && cached_data && authenticated?
      Thread.new { fetch_and_cache(path) }
      return cached_data
    end

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
        puts "[Brightspace API] POST Success: #{path}"
        true
      elsif response.code == '401' || response.code == '403'
        puts "[!] AUTH ERROR #{response.code} (POST): Cookie/Token likely expired."
        create_system_notification(
          id: "auth_error_post_#{response.code}",
          title: "Session Expired (#{response.code})",
          body: "A server update (POST) failed because your session expired. Please update your cookies.",
          urgency: 3
        )
        false
      else
        puts "[Brightspace API] POST Error #{response.code}: #{path}"
        false
      end
    rescue => e
      puts "[Brightspace API] POST Exception: #{e.message}"
      false
    end
  end

  def fetch_and_cache(path)
    return nil unless authenticated?
    
    # Optional: silence noisy expected 404s for discussions
    is_notoriously_noisy = path.include?('/discussions/') && path.include?('/threads/')

    puts "[Brightspace API] Fetching: #{path}" unless is_notoriously_noisy

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
      elsif response.code == '401' || response.code == '403'
        puts "[!] AUTH ERROR #{response.code}: Cookie/Token likely expired."
        create_system_notification(
          id: "auth_error_#{response.code}",
          title: "Session Expired (#{response.code})",
          body: "Your Brightspace session has expired or access was forbidden. Please update your cookies in config/connection.json or log in again.",
          urgency: 3
        )
        nil
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
