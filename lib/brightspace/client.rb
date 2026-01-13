require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'digest'
require 'fileutils'

class BrightspaceClient
  attr_accessor :token, :cookie_string, :host

  def initialize
    @host = ENV['BS_HOST']
    @client_id = ENV['BS_CLIENT_ID']
    @client_secret = ENV['BS_CLIENT_SECRET']
    @redirect_uri = ENV['BS_REDIRECT_URI'] || "http://localhost:4567/callback"
    @api_version = "1.40"
    @cache_dir = File.join(Dir.pwd, ".cache", "brightspace")
    FileUtils.mkdir_p(@cache_dir)
    
    # Try loading from cookies.txt (e.g. for pre-seeded dev)
    load_cookies_from_file if File.exist?('cookies.txt')
  end

  def load_cookies_from_file
    content = File.read('cookies.txt').strip
    return if content.empty?

    if content.start_with?('ey')
      @token = content
    else
      @cookie_string = content
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
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/assignments/")
  end

  def get_assignment(org_unit_id, assignment_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/assignments/#{assignment_id}")
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
      topics = get_discussion_topics(org_unit_id, f['ForumId']) || []
      topics.each { |t| t['ForumId'] = f['ForumId']; t['ForumName'] = f['Name'] }
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

  def get_discussion_thread(org_unit_id, forum_id, topic_id, thread_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/#{thread_id}")
  end

  # NEW: Fetch Thread Posts
  def get_thread_posts(org_unit_id, forum_id, topic_id, thread_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/#{forum_id}/topics/#{topic_id}/threads/#{thread_id}/posts/")
  end

  # NEW: Fetch all News for all courses efficiently
  def get_all_news(courses)
    news_items = []
    courses.each do |c|
      items = get_news(c['OrgUnit']['Id'])
      if items && items.is_a?(Array)
        items.each { |i| i['CourseName'] = c['OrgUnit']['Name']; i['CourseId'] = c['OrgUnit']['Id'] }
        news_items.concat(items)
      end
    end
    news_items.sort_by { |i| i['StartDate'] }.reverse.tap { |arr| arr.slice!(0..10) } # Just top 10
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
    # Filter out API version or variable parts if needed for better reuse, 
    # but simple hash of full path is safest.
    hash = Digest::MD5.hexdigest(path)
    File.join(@cache_dir, "#{hash}.json")
  end

  def read_cache(path)
    file = get_cache_path(path)
    if File.exist?(file)
      # Check if cache is older than 1 hour (optional, but good for offline)
      JSON.parse(File.read(file))
    end
  rescue
    nil
  end

  def write_cache(path, data)
    file = get_cache_path(path)
    File.write(file, data.to_json)
  rescue => e
    puts "Cache write error: #{e.message}"
  end

  def do_get(path)
    return read_cache(path) unless authenticated?

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
    http.read_timeout = 10 # 10 seconds timeout
    
    begin
      response = http.request(request)
      if response.code == '200'
        data = JSON.parse(response.body)
        write_cache(path, data)
        data
      else
        puts "API Error #{path}: #{response.code} #{response.message}"
        read_cache(path) # Fallback to cache on server error
      end
    rescue => e
      puts "Connection Error #{path}: #{e.message}. Falling back to cache."
      read_cache(path) # Fallback to cache on connection failure (offline)
    end
  end
end
