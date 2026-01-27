require 'sinatra/base'
require 'sinatra/activerecord'
require 'rack-flash'
require 'jwt'

class BaseController < Sinatra::Base
  register Sinatra::ActiveRecordExtension
  
  set :views, File.expand_path('../../views', __FILE__)
  set :public_folder, File.expand_path('../../public', __FILE__)
  
  enable :sessions
  use Rack::Flash, :sweep => true

  helpers CourseHelpers

  helpers do
    def configured?
      $client.authenticated?
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
      
      # If an override semester was provided, we re-calculate or just filter
      # In the current implementation, we'll let the service handle the default
      # and if someone specifically asks for another semester we could pass it.
      # For now, to maintain legacy signature:
      data
    end
  end

  before do
    @user_prefs ||= UserPreference.current
    Time.zone = @user_prefs.time_zone || "UTC"
    @iana_timezone = Time.zone.tzinfo.name

    # Config Check
    return if ['/setup', '/health', '/favicon.ico', '/logo.png', '/auth/login', '/login', '/docs', '/sync/status'].include?(request.path_info) || 
              request.path_info.start_with?('/public') || 
              request.path_info.start_with?('/api/') || 
              request.path_info.start_with?('/docs/')
    
    redirect '/setup' if !configured?

    @user ||= $client.get_who_am_i || { 'FirstName' => @user_prefs.display_name, 'LastName' => '' }
    
    if (@user_prefs.display_name == "User" || @user_prefs.display_name.nil?) && @user['DisplayName']
      @user_prefs.update(display_name: @user['DisplayName'])
    end
  end
  
  not_found do
    @error_title = "404 - Not Found"
    @error_message = "The page you are looking for does not exist."
    erb :error
  end

  error do
    @error = env['sinatra.error']
    @error_title = "500 - Server Error"
    @error_message = "An unexpected error occurred."
    puts "[Brilliant Error] #{@error.message}"
    puts @error.backtrace.first(10).join("\n")
    erb :error
  end
end
