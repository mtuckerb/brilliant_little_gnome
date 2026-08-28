require 'sinatra/base'
require 'sinatra/activerecord'
require 'rack-flash'
require 'jwt'

class BaseController < Sinatra::Base
  register Sinatra::ActiveRecordExtension
  
  set :views, File.expand_path('../../views', __FILE__)
  set :public_folder, File.expand_path('../../public', __FILE__)
  
  # Configuration shared across all controllers
  set :session_secret, ENV['SESSION_SECRET'] || 'brilliant_app_session_persistent_secret_12345'
  
  # Disable aggressive session hijacking protection which can cause issues in browsers
  set :protection, :except => [:session_hijacking, :remote_token]

  helpers CourseHelpers

  helpers do
    def configured?
      $client.authenticated? && !$client.degraded_mode
    end

    def flash
      f = env['x-rack.flash'] || env['rack.flash']
      f ||= {}
      f
    end

    def format_date(date, format = "%b %d, %Y %I:%M %p")
      return "Recently" if date.nil?
      d = date.is_a?(String) ? (Time.zone.parse(date) rescue nil) : date
      return date.to_s if d.nil?
      
      tz_name = @user_prefs&.time_zone || "UTC"
      d.in_time_zone(tz_name).strftime(format)
    rescue
      date.to_s
    end

    def truncate_text(text, max_length = 20)
      return text if text.nil? || text.length <= max_length
      text[0...max_length-1] + "…"
    end

    def page_url(page_num)
      new_params = params.dup
      new_params[:page] = page_num
      query_string = Rack::Utils.build_query(new_params)
      "#{request.path_info}?#{query_string}"
    end

    def generate_jwt_token(payload)
      payload[:uid] ||= @user_prefs.brightspace_uid
      payload[:bs_user_id] ||= @user_prefs.brightspace_user_id
      expiry = Time.now.to_i + (3600 * 24 * 30) # 30 days
      payload[:exp] = expiry
      JWT.encode(payload, @user_prefs.jwt_secret, 'HS256')
    end

    def decode_jwt_token(token)
      begin
        @decoded_token = JWT.decode(token, @user_prefs.jwt_secret, true, { algorithm: 'HS256' })[0]
        @decoded_token
      rescue JWT::ExpiredSignature
        halt 401, { error: "Token has expired" }.to_json
      rescue JWT::DecodeError => e
        halt 401, { error: "Invalid token: #{e.message}" }.to_json
      end
    end

    def validate_api_access!
      # 1. Internal Session Fallback (e.g. from web frontend)
      return true if configured?

      # 2. Check for API Key in Header (X-API-Key) or Param
      api_key = request.env['HTTP_X_API_KEY'] || params[:api_key]
      if api_key.present? && @user_prefs.api_key.present? && api_key == @user_prefs.api_key
        return true
      end

      # 3. Check for JWT Token in Authorization Header or Param
      token = nil
      auth_header = request.env['HTTP_AUTHORIZATION']
      if auth_header && auth_header.start_with?('Bearer ')
        token = auth_header.split(' ').last
      elsif params[:api_key].present?
        # If it's not the static API key, try it as a JWT
        token = params[:api_key]
      end

      if token.present?
        begin
          @decoded_token = decode_jwt_token(token)
          return true if @decoded_token
        rescue => e
          # decode_jwt_token handles its own halts
        end
      end

      halt 401, { error: "Unauthorized: API access required" }.to_json
    end

    def get_dashboard_summary_data(overview_semester = nil)
      data = Brilliant::DashboardService.get_summary_data(@user_prefs)
      
      # Fix for courses that haven't started (0% grade)
      # Ensure data is consistent even if the service returned raw numbers
      if data && data[:semester_grades]
        data[:semester_grades].each do |sg|
          if sg[:stats] && sg[:stats][:item_count] == 0
            sg[:stats][:score] = nil
          end
        end
      end

      data
    end

    def extract_course_info(full_name, org_unit_id = nil)
      CourseHelpers.extract_course_info(full_name, org_unit_id)
    end
  end

  before do
    # ULTIMATE DEBUG
    puts "[BASE before] path_info: #{request.path_info}, env['PATH_INFO']: #{env['PATH_INFO']}, params: #{params.inspect}"
    
    @user_prefs ||= UserPreference.current
    Time.zone = @user_prefs.time_zone || "UTC"
    @iana_timezone = Time.zone.tzinfo.name

    # Improved Course Context resolution for middleware
    # 1. Check for explicit params (if already matched by the current controller)
    @course_id = params[:id] || params[:course_id]
    
    # 2. Extract from URL using Regex on PATH_INFO (reliable for all middleware layers)
    if @course_id.to_s.empty?
      full_path = env['PATH_INFO'] || request.path_info
      # Match patterns like /course/12345 or /api/v1/courses/12345
      if full_path =~ %r{/courses?/(\d+)}
        @course_id = $1
      end
    end

    if @course_id.present?
      @course_id = @course_id.to_s.gsub(/[^0-9]/, '') if @course_id.to_s.match?(/^\d+$/)
      @course ||= Course.find_by(org_unit_id: @course_id.to_s)
      @course_name = @course&.display_name || "Course #{@course_id}"
    end

    # Config Check
    return if ['/setup', '/health', '/favicon.ico', '/logo.png', '/auth/login', '/auth/magic', '/login', '/callback', '/docs', '/openapi.yaml', '/sync/status'].include?(request.path_info) ||
              request.path_info.start_with?('/public') || 
              request.path_info.start_with?('/api/') || 
              request.path_info.start_with?('/auth/') ||
              request.path_info.start_with?('/docs/')
    
    redirect '/setup' if !configured?

    @user ||= $client.get_who_am_i || { 'FirstName' => @user_prefs.display_name, 'LastName' => '' }
    
    if (@user_prefs.display_name == "User" || @user_prefs.display_name.nil?) && @user['DisplayName']
      @user_prefs.update(display_name: @user['DisplayName'])
    end
  end
  
  # Removed not_found and error blocks to prevent middleware interference
  # These should be defined in the main app class
end
