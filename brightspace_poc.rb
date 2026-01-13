#!/usr/bin/env ruby

require 'uri'
require 'net/http'
require 'json'
require 'base64'
require 'cgi'

# ==========================================
# Configuration
# ==========================================
# You can set these via environment variables or edit them here
HOST          = ENV['BS_HOST']          # e.g. "myschool.brightspace.com"
CLIENT_ID     = ENV['BS_CLIENT_ID']     # OAuth2 Client ID
CLIENT_SECRET = ENV['BS_CLIENT_SECRET'] # OAuth2 Client Secret
REDIRECT_URI  = ENV['BS_REDIRECT_URI'] || 'https://localhost/callback'

# Helper for color output
def log(msg, type = :info)
  colors = { info: "\e[36m", success: "\e[32m", error: "\e[31m", reset: "\e[0m" }
  puts "#{colors[type]}#{msg}#{colors[:reset]}"
end

class BrightspaceClient
  def initialize(host, client_id, client_secret, redirect_uri)
    @host = host
    @client_id = client_id
    @client_secret = client_secret
    @redirect_uri = redirect_uri
    @token = nil
    @api_version = "1.40" # Safe default, can be bumped if needed
  end

  # ==========================================
  # Authentication (OAuth 2.0 Authorization Code Flow)
  # ==========================================

  def authenticate
    # Option 1: Load from cookies.txt if present
    if File.exist?('cookies.txt')
      content = File.read('cookies.txt').strip
      unless content.empty?
        log "Loading credentials from cookies.txt...", :info
        if content.start_with?('ey')
           @token = content
           log "Loaded Bearer Token from file.", :success
        else
           @cookie_string = content
           log "Loaded Session Cookies from file.", :success
        end
        return
      end
    end

    # Option 2: Bypass OAuth if credentials provided via Env Var
    if ENV['BS_TOKEN']
      log "Using pre-supplied API token from environment...", :info
      @token = ENV['BS_TOKEN']
      return
    elsif ENV['BS_COOKIE']
      log "Using pre-supplied Session Cookie from environment...", :info
      @cookie_string = ENV['BS_COOKIE']
      return
    end

    # Step 1: Generate Authorization URL
    auth_url = "https://#{@host}/oauth2/auth?" + URI.encode_www_form({
      response_type: 'code',
      client_id: @client_id,
      redirect_uri: @redirect_uri,
      scope: 'core:*:* enrollments:*:* content:*:* grades:*:*' 
    })

    log "\n=== Authentication Required ==="
    log "Option 1: Paste a Bearer Token (starts with 'ey...')"
    log "Option 2: Paste your Session Cookies (starts with 'd2lSessionVal=...')"
    log "Option 3: Press Enter to perform standard OAuth login (requires Client ID/Secret)"
    
    print "Input > "
    input = gets.chomp.strip

    if input.start_with?('ey')
       @token = input
       log "Using manual Bearer Token.", :success
       return
    elsif input.include?('d2lSessionVal') || input.include?('JSESSIONID')
       @cookie_string = input
       log "Using manual Session Cookies.", :success
       return
    elsif !input.empty?
       # Fallback assume token
       @token = input
       log "Assuming input is a token.", :success
       return
    end

    log "Please open the following URL in your browser to authorize the application:"
    puts auth_url
    puts "\nAfter logging in and authorizing, you will be redirected to a URL that looks like:"
    puts "#{@redirect_uri}?code=YOUR_CODE_HERE"
    puts "\nPaste the 'code' value here:"
    
    print "> "
    auth_code = gets.chomp.strip

    if auth_code.empty?
      log "No code provided. Exiting.", :error
      exit 1
    end

    # Step 2: Exchange Code for Access Token
    log "Exchanging code for token..."
    
    uri = URI("https://#{@host}/core/connect/token")
    request = Net::HTTP::Post.new(uri)
    request.content_type = 'application/x-www-form-urlencoded'
    
    # Basic Auth Header for the token endpoint
    auth_str = Base64.strict_encode64("#{@client_id}:#{@client_secret}")
    request['Authorization'] = "Basic #{auth_str}"
    
    request.set_form_data({
      'grant_type' => 'authorization_code',
      'redirect_uri' => @redirect_uri,
      'code' => auth_code
    })

    response = Net::HTTP.start(uri.hostname, uri.port, use_ssl: true) do |http|
      http.request(request)
    end

    if response.code == '200'
      data = JSON.parse(response.body)
      @token = data['access_token']
      # In a real app, you would save data['refresh_token'] too
      log "Authentication successful!", :success
    else
      log "Authentication failed: #{response.body}", :error
      exit 1
    end
  end

  # ==========================================
  # API Requests
  # ==========================================

  def get_my_enrollments
    ensure_authenticated!
    
    # Filter strictly for Course Offerings (Type ID 3 is usually Course Offering, but we can filter by name too)
    # ?orgUnitTypeId=3 helps, but some systems vary. We'll fetch all and filter client-side for safety.
    path = "/d2l/api/lp/#{@api_version}/enrollments/myenrollments/"
    
    log "Fetching enrollments..."
    
    response = make_request('GET', path)
    
    if response.code == '200'
      data = JSON.parse(response.body)
      items = data['Items'] || data
      
      # Filter for typical courses (ignore Departments, Semesters, etc if possible)
      courses = items.select { |i| i.dig('OrgUnit', 'Type', 'Code') == 'Course Offering' || i.dig('OrgUnit', 'Type', 'Name') == 'Course Offering' }
      
      # If strict filtering hides everything, fallback to showing all
      courses = items if courses.empty?

      log "Found #{courses.count} Active Courses (out of #{items.count} total enrollments):", :success
      puts "\n"
      
      # Print Header
      printf "%-10s | %-15s | %-50s\n", "ID", "CODE", "NAME"
      puts "-" * 80
      
      courses.each do |enrollment|
        org = enrollment['OrgUnit']
        # Simple formatting
        id = org['Id']
        code = (org['Code'] || "")[0..14] # Truncate code
        name = (org['Name'] || "")[0..49] # Truncate name
        
        printf "%-10s | %-15s | %-50s\n", id, code, name
      end
      puts "-" * 80
      puts "\n"
      
      return courses
    else
      log "Failed to fetch enrollments: #{response.code} - #{response.body}", :error
      return []
    end
  end

  def get_who_am_i
    ensure_authenticated!
    # 'lp' API 
    path = "/d2l/api/lp/#{@api_version}/users/whoami"
    
    response = make_request('GET', path)
    if response.code == '200'
      user = JSON.parse(response.body)
      log "Logged in as: #{user['FirstName']} #{user['LastName']} (#{user['UniqueName']})", :success
    else 
      log "WhoAmI check failed: #{response.code} #{response.message}", :error
      if @cookie_string
         log "Debug: Sent Cookie Header length: #{@cookie_string.length}", :info
      end
    end
  end
  
  private

  def ensure_authenticated!
    unless @token || @cookie_string
      log "Not authenticated.", :error
      exit 1
    end
  end

  def make_request(method, path)
    uri = URI("https://#{@host}#{path}")
    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true

    case method
    when 'GET'
      request = Net::HTTP::Get.new(uri)
    when 'POST'
      request = Net::HTTP::Post.new(uri)
    end

    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string
    end
    
    request['Accept'] = 'application/json'

    http.request(request)
  end
end

# ==========================================
# Main Execution
# ==========================================

if __FILE__ == $0
  
  # Check Env Vars
  missing = []
  missing << 'BS_HOST' unless ENV['BS_HOST']
  missing << 'BS_CLIENT_ID' unless ENV['BS_CLIENT_ID']
  missing << 'BS_CLIENT_SECRET' unless ENV['BS_CLIENT_SECRET']
  
  if missing.any?
    puts "Error: Missing required environment variables."
    puts "Please set the following:"
    missing.each { |v| puts "  - #{v}" }
    puts "\nUsage:"
    puts "  export BS_HOST='myschool.brightspace.com'"
    puts "  export BS_CLIENT_ID='your_client_id'"
    puts "  export BS_CLIENT_SECRET='your_client_secret'"
    puts "  export BS_REDIRECT_URI='https://localhost/callback' (Optional, defaults locally)"
    puts "  ruby brightspace_poc.rb"
    exit 1
  end

  client = BrightspaceClient.new(
    ENV['BS_HOST'],
    ENV['BS_CLIENT_ID'],
    ENV['BS_CLIENT_SECRET'],
    ENV['BS_REDIRECT_URI'] || 'https://localhost/callback'
  )

  client.authenticate
  puts "\n"
  client.get_who_am_i
  puts "\n"
  client.get_my_enrollments
end
