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
      d = date.is_a?(String) ? (Time.parse(date) rescue nil) : date
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
      # 1. Check for API Key in Header (X-API-Key) or Param
      api_key = request.env['HTTP_X_API_KEY'] || params[:api_key]
      if api_key.present? && @user_prefs.api_key.present? && api_key == @user_prefs.api_key
        return true
      end

      # 2. Check for JWT Token in Authorization Header
      auth_header = request.env['HTTP_AUTHORIZATION']
      if auth_header && auth_header.start_with?('Bearer ')
        token = auth_header.split(' ').last
        begin
          @decoded_token = decode_jwt_token(token)
          return true if @decoded_token
        rescue => e
          # decode_jwt_token handles its own halts
        end
      end

      # 3. Internal Session Fallback
      if session[:local_authenticated] == true || configured?
        return true
      end

      halt 401, { error: "Unauthorized: API access required" }.to_json
    end

    def get_dashboard_summary_data(overview_semester = nil)
      @courses = Course.all.order(is_pinned: :desc, last_accessed_at: :desc)
      all_semesters = @courses.map(&:semester).compact.uniq.sort_by { |s| semester_weight(s) }
      latest_semester = all_semesters.last
      overview_semester ||= @user_prefs.default_semester || latest_semester
      
      semester_grades = []
      total_weighted_points = 0.0
      total_units_count = 0
      cumulative_points_earned = 0.0
      cumulative_points_possible = 0.0
      
      if overview_semester
        overview_weight = semester_weight(overview_semester)
        @courses.each do |c|
          next unless c.semester
          c_weight = semester_weight(c.semester)
          next if c_weight > overview_weight
          
          stats = Grade.calculate_weighted_total(c.org_unit_id) rescue nil
          next unless stats
          
          if c.semester == overview_semester
            semester_grades << { course: c, stats: stats }
          end

          target = c.target_grade || 93.0
          score_to_use = (c_weight == overview_weight) ? 
            (stats[:all_possible_points] > 0 ? ((stats[:total_points_earned] + (stats[:all_possible_points] - stats[:total_points_possible]) * (target/100.0)) / stats[:all_possible_points] * 100.0) : target) : 
            stats[:score]

          if stats[:total_points_possible] > 0 || c_weight == overview_weight
            cumulative_points_earned += stats[:total_points_earned]
            cumulative_points_possible += stats[:total_points_possible]
            
            course_units = c.units || 3
            total_weighted_points += (Grade.to_gpa(score_to_use) * course_units)
            total_units_count += course_units
          end
        end
      end

      historic_gpa = @user_prefs.historic_gpa || 0.0
      historic_units = @user_prefs.historic_units || 0
      overall_gpa = (total_units_count + historic_units) > 0 ? 
        ((total_weighted_points + (historic_gpa * historic_units)) / (total_units_count + historic_units)) : 0.0

      total_max_weighted_points = (historic_gpa * historic_units) + total_weighted_points - (semester_grades.sum { |sg| Grade.to_gpa(sg[:stats][:score]) * (sg[:course].units || 3) })
      semester_grades.each do |sg|
        course_units = sg[:course].units || 3
        max_course_gpa = Grade.to_gpa(sg[:stats][:max_potential_score])
        total_max_weighted_points += (max_course_gpa * course_units)
      end
      max_potential_gpa = (total_units_count + historic_units) > 0 ? (total_max_weighted_points / (total_units_count + historic_units)) : 0.0

      {
        overall_gpa: overall_gpa,
        max_potential_gpa: max_potential_gpa,
        semester: overview_semester,
        semester_grades: semester_grades,
        all_semesters: all_semesters,
        cumulative_points_earned: cumulative_points_earned,
        cumulative_points_possible: cumulative_points_possible
      }
    end
  end

  before do
    @user_prefs ||= UserPreference.current
    Time.zone = @user_prefs.time_zone || "UTC"

    # 1. Passcode Gate
    if @user_prefs.web_access_passcode.present?
      unless ['/local_login', '/favicon.ico', '/logo.png', '/health', '/api/v1/token'].include?(request.path_info) || 
             request.path_info.start_with?('/public') || 
             request.path_info.start_with?('/api/')
        
        redirect '/local_login' if session[:local_authenticated] != true
      end
    end

    # 2. Config Check
    return if ['/setup', '/health', '/favicon.ico', '/logo.png', '/auth/login', '/login', '/docs', '/sync/status', '/local_login'].include?(request.path_info) || 
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
