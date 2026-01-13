require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'
require 'time'

class BrightspaceClient
  attr_accessor :token, :cookie_string, :host, :user_display_name

  def initialize
    @host = ENV['BS_HOST'] || "courses.maine.edu"
    @client_id = ENV['BS_CLIENT_ID']
    @client_secret = ENV['BS_CLIENT_SECRET']
    @redirect_uri = ENV['BS_REDIRECT_URI'] || "http://localhost:4567/callback"
    @api_version = "1.40"
    @sync_lock = Mutex.new
    @syncing = false
    
    # Try loading from cookies.txt (e.g. for pre-seeded dev)
    load_cookies_from_file if File.exist?('cookies.txt')
  end

  def sync_all_courses_proactively
    return if @syncing
    
    Thread.new do
      @sync_lock.synchronize { @syncing = true }
      begin
        puts "Starting proactive sync (slow mode)..."
        courses = get_enrollments 
        user = get_who_am_i
        
        # Sync Notifications first
        sync_notifications(courses, user)

        courses.each do |c|
          course_id = c['OrgUnit']['Id']
          
          # Sync Core
          get_toc(course_id)
          sleep 0.5
          get_assignments(course_id)
          sleep 0.5
          
          # Sync Discussions
          forums = get_discussions(course_id) || []
          forums.each do |f|
            topics_data = get_discussion_topics(course_id, f['ForumId'])
            topics = topics_data.is_a?(Hash) ? (topics_data['Items'] || []) : (topics_data || [])

            topics.each do |t|
              get_discussion_topic(course_id, f['ForumId'], t['TopicId'])
              sleep 1.0 # Very slow
              
              threads_data = get_discussion_threads(course_id, f['ForumId'], t['TopicId']) || []
              threads = threads_data.is_a?(Hash) ? (threads_data['Items'] || []) : threads_data
            end
          end
        end
        puts "Proactive sync complete."
      rescue => e
        puts "Sync error: #{e.message}"
        puts e.backtrace
      ensure
        @sync_lock.synchronize { @syncing = false }
      end
    end
  end

  def sync_notifications(courses, user)
    puts "[Brightspace API] Syncing notifications to DB..."
    
    # Get unified feed (News)
    feed_items = get_unified_feed(courses)
    feed_items.each do |item|
      upsert_notification(item)
    end

    # Get grades
    grade_items = get_recent_grades_notifications(courses)
    grade_items.each do |item|
      upsert_notification(item)
    end

    # Get discussions
    disc_items = get_discussion_notifications(courses, user['Identifier'])
    disc_items.each do |item|
      upsert_notification(item)
    end
    
    puts "[Brightspace API] Notification sync complete."
  end

  def upsert_notification(data)
    # Map symbols to strings for ActiveRecord
    n = Notification.find_or_initialize_by(external_id: data[:id].to_s, course_id: data[:course_id].to_s)
    n.notification_type = data[:type]
    n.title = data[:title]
    n.body = data[:body]
    n.date = Time.parse(data[:date]) rescue Time.now
    n.course_name = data[:course_name]
    n.semester = data[:semester]
    n.urgency = data[:urgency]
    n.is_personal = data[:is_personal]
    n.url = data[:url]
    n.save!
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

  def get_discussion_topics(org_unit_id, forum_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/")
  end

  def get_discussion_topic(org_unit_id, forum_id, topic_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}")
  end

  def get_discussion_threads(org_unit_id, forum_id, topic_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/")
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

  def get_news(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/news/")
  end

  def get_unified_feed(courses = [])
    # Used for title mapping
    course_map = courses.each_with_object({}) { |c, h| h[c['OrgUnit']['Id'].to_s] = c['OrgUnit']['Name'] }

    feed = do_get("/d2l/api/lp/#{@api_version}/feed/") || []
    
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
          date: Time.now.iso8601,
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
      elsif response.code == '401'
        puts "[!] AUTH EXPIRED: Need to re-login or refresh tokens."
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
