require 'uri'
require 'net/http'
require 'json'
require 'base64'

class BrightspaceClient
  attr_accessor :token, :cookie_string

  def initialize
    @host = ENV['BS_HOST']
    @client_id = ENV['BS_CLIENT_ID']
    @client_secret = ENV['BS_CLIENT_SECRET']
    @redirect_uri = ENV['BS_REDIRECT_URI'] || "http://localhost:4567/callback"
    @api_version = "1.40"
    
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
    end
  end

  def get_toc(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/content/toc")
  end

  def get_assignments(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/assignments/")
  end

  def get_grades(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/grades/values/myGradeValues/")
  end

  def get_discussions(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/")
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
    
    http.request(request)
  end

  private

  def do_get(path)
    return nil unless authenticated?

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
    
    response = http.request(request)
    
    if response.code == '200'
      JSON.parse(response.body)
    else
      puts "Error parsing #{path}: #{response.code} #{response.message}"
      nil
    end
  end
end
