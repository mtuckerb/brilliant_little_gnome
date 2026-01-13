require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'

class BrightspaceClient
  attr_accessor :token, :cookie_string, :host

  def initialize
    @host = ENV['BS_HOST'] || "courses.maine.edu"
    @client_id = ENV['BS_CLIENT_ID']
    @client_secret = ENV['BS_CLIENT_SECRET']
    @redirect_uri = ENV['BS_REDIRECT_URI'] || "http://localhost:4567/callback"
    @api_version = "1.40"
    @cache_dir = File.join(Dir.pwd, ".cache", "brightspace")
    @sync_lock = Mutex.new
    @syncing = false
    FileUtils.mkdir_p(@cache_dir)
    
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
              sleep 0.5
              
              threads_data = get_discussion_threads(course_id, f['ForumId'], t['TopicId']) || []
              threads = threads_data.is_a?(Hash) ? (threads_data['Items'] || []) : threads_data
              
              threads.each do |th|
                if th.is_a?(Hash) && th['ThreadId']
                   # ONLY sync posts for things we don't have yet to reduce noise
                   posts_path = "/d2l/api/le/#{@api_version}/#{course_id}/discussions/forums/#{f['ForumId']}/topics/#{t['TopicId']}/threads/#{th['ThreadId']}/posts/"
                   unless read_cache(posts_path)
                     get_thread_posts(course_id, f['ForumId'], t['TopicId'], th['ThreadId'])
                     sleep 1.0 # Very slow to avoid attention
                   end
                end
              end
            end
          end
        end
        puts "Proactive sync complete."
      rescue => e
        puts "Sync error: #{e.message}"
      ensure
        @sync_lock.synchronize { @syncing = false }
      end
    end
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
    do_get("/d2l/api/lp/#{@api_version}/users/whoami")
  end

  def get_enrollments
    response = do_get("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/")
    return [] unless response
    
    items = response['Items'] || response
    items.select do |i| 
      (i.dig('OrgUnit', 'Type', 'Code') == 'Course Offering' || 
       i.dig('OrgUnit', 'Type', 'Name') == 'Course Offering')
    end.sort_by { |i| i['PinDate'] ? 0 : 1 } # Pinned first (PinDate exists if pinned)
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
        
        # Enrich Topic with thread stats if missing
        threads_data = get_discussion_threads(org_unit_id, f['ForumId'], t['TopicId']) || []
        threads = threads_data.is_a?(Hash) ? (threads_data['Items'] || []) : (threads_data || [])
        
        # Calculate stats for the user if the server didn't provide them
        t['ThreadCount'] ||= threads.size
        # PostCount is total replies + initial posts in threads
        t['PostCount'] ||= threads.map { |th| (th['ReplyCount'] || 0) + 1 }.sum
        
        # Find latest activity date
        last_modified = threads.map { |th| th['LastModified'] }.compact.max
        t['LastPostDate'] ||= last_modified
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

  def get_discussion_threads(org_unit_id, forum_id, topic_id, force_refresh: false)
    # Attempt 1: Standard forum-based path with trailing slash
    path1 = "/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/"
    data = fetch_and_cache_with_fallback(path1, force_refresh: force_refresh)
    return data if data && !data.empty? && !(data.is_a?(Hash) && data['Items']&.empty?)

    # Attempt 2: Direct topic path (fallback)
    path2 = "/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/topics/#{topic_id}/threads/"
    data = fetch_and_cache_with_fallback(path2, force_refresh: force_refresh)
    return data if data && !data.empty? && !(data.is_a?(Hash) && data['Items']&.empty?)

    # NEW ATTEMPT: If threads are 404/Empty, we fetch ALL POSTS and group them by ThreadId
    # This is a powerful fallback for environments that lock down the thread list but allow post access.
    posts_path = "/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/posts/"
    all_posts = fetch_and_cache_with_fallback(posts_path, force_refresh: force_refresh)
    
    if all_posts && all_posts.is_a?(Array) && !all_posts.empty?
      puts "[Brightspace API] Reconstructing thread list from #{all_posts.size} posts..."
      
      # Group by ThreadId
      threads_map = {}
      all_posts.each do |p|
        tid = p['ThreadId']
        next unless tid
        
        threads_map[tid] ||= {
          'ThreadId' => tid,
          'TopicId' => p['TopicId'],
          'ForumId' => p['ForumId'],
          'Subject' => p['Subject'], 
          'PostingUserDisplayName' => p['PostingUserDisplayName'],
          'LastModified' => p['DatePosted'],
          'ReplyCount' => 0,
          'IsPinned' => false
        }
        
        # If it's the root post (ParentPostId nil or empty), use its subject/date
        if p['ParentPostId'].nil? || p['ParentPostId'] == 0
          threads_map[tid]['Subject'] = p['Subject']
          threads_map[tid]['DatePosted'] = p['DatePosted']
          threads_map[tid]['PostingUserDisplayName'] = p['PostingUserDisplayName']
        else
          threads_map[tid]['ReplyCount'] += 1
          # Update LastModified if this post is newer
          if p['DatePosted'] > (threads_map[tid]['LastModified'] || "")
             threads_map[tid]['LastModified'] = p['DatePosted']
          end
        end
      end
      
      return { 'Items' => threads_map.values.sort_by { |th| th['LastModified'] || "" }.reverse }
    end

    nil
  end

  # Helper to handle the actual fetching without recursive do_get calls
  def fetch_and_cache_with_fallback(path, force_refresh: false)
    cached = read_cache(path)
    return cached if cached && !force_refresh
    
    # We use fetch_and_cache directly to skip the Thread.new backgrounding in do_get
    # for these specific fallbacks so we can check results immediately.
    fetch_and_cache(path)
  end

  def get_discussion_evaluation(org_unit_id, forum_id, topic_id)
    # Explicitly check if forum_id is nil to avoid malformed URLs
    if forum_id.nil? || forum_id.to_s.empty?
      puts "[Brightspace API] WARNING: forum_id is nil in get_discussion_evaluation. Using fallback..."
      # Some versions might allow topics without forum context in the URL
      return do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/topics/#{topic_id}/evaluations/myEvaluation")
    end
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/evaluations/myEvaluation")
  end

  def get_discussion_thread(org_unit_id, forum_id, topic_id, thread_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/#{thread_id}")
  end

  def get_thread_posts(org_unit_id, forum_id, topic_id, thread_id)
    # Check if we already have the full post list for this topic in cache
    # This avoids redundant 404s if threads endpoint is disabled
    posts_path = "/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/posts/"
    all_posts = read_cache(posts_path)
    
    if all_posts && all_posts.is_a?(Array)
      filtered = all_posts.select { |p| p['ThreadId'].to_s == thread_id.to_s }
      return { 'Items' => filtered } unless filtered.empty?
    end

    path = "/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/#{thread_id}/posts/"
    data = do_get(path)
    
    # Fallback: Topic-only endpoint
    if data.nil? || (data.is_a?(Hash) && data['Items']&.empty?)
      data = do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/topics/#{topic_id}/threads/#{thread_id}/posts/")
    end
    data
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

  def portal_url_for(type, params)
    course_id = params[:course_id]
    case type
    when :assignment
      "https://#{@host}/d2l/lms/dropbox/user/folder_submit_files.d2l?db=#{params[:id]}&ou=#{course_id}"
    when :discussion_topic
      "https://#{@host}/d2l/le/1.40/#{course_id}/discussions/topics/#{params[:id]}/View"
    when :discussion_thread
      "https://#{@host}/d2l/le/1.40/#{course_id}/discussions/threads/#{params[:id]}/View"
    when :course_home
      "https://#{@host}/d2l/home/#{course_id}"
    else
      "https://#{@host}"
    end
  end

  private

  def get_cache_path(path)
    hash = Digest::MD5.hexdigest(path)
    File.join(@cache_dir, "#{hash}.json")
  end

  def read_cache(path)
    file = get_cache_path(path)
    if File.exist?(file)
      JSON.parse(File.read(file))
    end
  rescue
    nil
  end

  def cache_metadata(path)
    file = get_cache_path(path)
    return nil unless File.exist?(file)
    { mtime: File.mtime(file) }
  end

  def write_cache(path, data)
    return unless data.is_a?(Hash) || data.is_a?(Array)
    if data.is_a?(Hash) && (data.key?('Errors') || data.key?('ErrorMessage'))
      return 
    end

    file = get_cache_path(path)
    File.write(file, data.to_json)
  rescue => e
    puts "Cache write error: #{e.message}"
  end

  def do_get(path, force_refresh: false)
    cached_data = read_cache(path)
    metadata = cache_metadata(path)
    
    is_fresh = metadata && (Time.now - metadata[:mtime] < 600) # 10 mins

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
    puts "[Brightspace API] Fetching: #{path}"

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
        puts "[!] 404 NOT FOUND: #{path}"
        nil
      else
        puts "[!] API ERROR #{response.code}: #{path}"
        read_cache(path)
      end
    rescue => e
      puts "[!] API EXCEPTION: #{e.message}"
      read_cache(path)
    end
  end
end
