require 'bundler/setup'
require 'fileutils'
require 'uri'
require 'cgi'

require 'securerandom'

# Handle --headless flag before Sinatra/Bundler parses ARGV
$headless_mode = ARGV.delete('--headless')

# Pre-emptively rescue EPIPE on standard streams to prevent sidecar crashes during pipe-cleanup
def $stderr.write(data)
  super rescue nil
end
def $stdout.write(data)
  super rescue nil
end

Bundler.require(:default)

require 'sinatra'
require 'sinatra/activerecord'
require 'active_support/time'
require 'time'
require 'zip'
require 'tempfile'
require 'icalendar'
require 'rack-flash'

require_relative 'lib/brilliant/client'
require_relative 'lib/brilliant/auth_helper'
require_relative 'helpers/course_helpers'
require_relative 'models/notification'
require_relative 'models/api_cache'
require_relative 'models/download_job'
require_relative 'models/user_preference'
require_relative 'models/course'
require_relative 'models/content_module'
require_relative 'models/content_item'
require_relative 'models/assignment'
require_relative 'models/discussion_forum'
require_relative 'models/discussion_topic'
require_relative 'models/discussion_thread'
require_relative 'models/discussion_post'
require_relative 'models/grade'

# --- Runtime Database Initialization ---
# Move this early so models can be used in configure blocks
begin
  puts "[Brilliant] Starting Database Initialization..."
  
  # Increase connection pool size to handle background sync threads
  db_config = {}
  
  # If using absolute path in DATABASE_URL, ensure parent directory exists
  if ENV['DATABASE_URL'] && ENV['DATABASE_URL'].start_with?('sqlite3://')
    # Correctly parse URI and handle %20 specifically
    uri = URI.parse(ENV['DATABASE_URL'])
    db_real_path = CGI.unescape(uri.path)
    puts "[Brilliant] Using DATABASE_URL: #{db_real_path}"
    FileUtils.mkdir_p(File.dirname(db_real_path))
    
    db_config = {
      adapter: "sqlite3",
      database: db_real_path,
      pool: 20,
      timeout: 5000
    }
  else
    # Ensure the local directory exists
    puts "[Brilliant] Using default development database"
    FileUtils.mkdir_p("db")
    db_path = File.expand_path("db/development.sqlite3")
    puts "[Brilliant] Database path: #{db_path}"
    db_config = {
      adapter: "sqlite3",
      database: "db/development.sqlite3",
      pool: 20,
      timeout: 5000
    }
  end

  ActiveRecord::Base.establish_connection(db_config)
  puts "[Brilliant] Connection established"

  # Optimization: Enable WAL mode for SQLite to handle concurrency with self-healing check
  begin
    ActiveRecord::Base.connection.execute("PRAGMA journal_mode=WAL;")
    ActiveRecord::Base.connection.execute("PRAGMA synchronous=NORMAL;")
    ActiveRecord::Base.connection.execute("PRAGMA busy_timeout=5000;")
  rescue SQLite3::NotADatabaseException => e
    puts "[Brilliant] Critical Error: Database file is not a valid SQLite database (header corrupt)."
    
    # Self-healing: if the file is truly not a database, it is useless.
    # We will attempt to delete it and let initialization start over once.
    db_file = db_config[:database]
    if db_file && File.exist?(db_file) && !@retried_init
      puts "[Brilliant] Attempting self-healing by removing corrupted database: #{db_file}"
      ActiveRecord::Base.remove_connection
      File.delete(db_file) rescue nil
      Dir.glob("#{db_file}-*").each { |f| File.delete(f) rescue nil }
      @retried_init = true
      retry
    end
    raise e
  end
  
  # Run migrations if pending
  # In AR 5.2 MigrationContext takes only the migrations path
  context = ActiveRecord::MigrationContext.new("db/migrate")
  if context.needs_migration?
    puts "[Brilliant] Pending migrations detected. Automating migration..."
    context.migrate
    puts "[Brilliant] Database schema is now up to date."
  end
  puts "[Brilliant] Database Initialization Complete"
rescue => e
  puts "[Brilliant] Database initialization warning: #{e.message}"
  puts e.backtrace.first(5).join("\n")
end

# Basic Configuration
set :port, 4567
set :views, File.join(File.dirname(__FILE__), 'views')
set :public_folder, File.join(File.dirname(__FILE__), 'public')

# PID Management for Electron Sidecar
if ENV['BRILLIANT_DATA_DIR']
  pid_path = File.join(ENV['BRILLIANT_DATA_DIR'], 'ruby_sidecar.pid')
  File.write(pid_path, Process.pid.to_s)
  at_exit { File.delete(pid_path) if File.exist?(pid_path) }
end

# API Listening Configuration
configure do
  begin
    prefs = UserPreference.current
    if prefs.api_listen_all
      set :bind, '0.0.0.0'
    else
      set :bind, '0.0.0.0'
    end
  rescue => e
    puts "Warning: Could not load API preferences during boot: #{e.message}"
    set :bind, '127.0.0.1'
  end
end

# Initialize Client
$client = BrilliantClient.new
# Helpers
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

  def validate_api_access!
    unless @user_prefs.api_enabled
      halt 403, { error: "REST API is disabled in settings" }.to_json
    end

    return if @user_prefs.api_key.blank?

    key = request.env['HTTP_X_API_KEY'] || request.env['HTTP_AUTHORIZATION']&.split(' ')&.last
    if key != @user_prefs.api_key
      halt 401, { error: "Invalid or missing API Key" }.to_json
    end
  end
end

# System Check
get '/health' do
  status 200
  "OK"
end

# Serve API Documentation
get '/docs' do
  redirect '/docs/'
end

get '/docs/' do
  send_file File.join(settings.root, 'docs', 'index.html')
end

get '/docs/:filename' do |filename|
  # Basic security to prevent path traversal
  filename = File.basename(filename)
  send_file File.join(settings.root, 'docs', filename)
end

# ==========================================
# Routes
# ==========================================

before do
  # Memoize preferences for all requests so they are available in layouts/helpers
  @user_prefs ||= UserPreference.current

  # Allow access to setup and public files without being "configured"
  return if ['/setup', '/favicon.ico', '/logo.png', '/auth/login', '/docs', '/sync/status'].include?(request.path_info) || 
            request.path_info.start_with?('/public') || 
            request.path_info.start_with?('/api/') || 
            request.path_info.start_with?('/docs/')
  
  if !configured?
    redirect '/setup'
  end

  # Only fetch whoami if we haven't already in this request
  # and use a shorter timeout for the background refresh to reduce noise
  @user ||= $client.get_who_am_i || { 'FirstName' => @user_prefs.display_name, 'LastName' => '' }
  
  # Auto-fetch name from Brightspace if we still have the default or empty
  if (@user_prefs.display_name == "User" || @user_prefs.display_name.nil?) && @user['DisplayName']
    if @user && @user['DisplayName']
      @user_prefs.update(display_name: @user['DisplayName'])
    end
  end
end

get '/settings' do
  @active_tab = 'settings'
  @host = $client.host
  erb :settings
end

post '/settings' do
  @user_prefs.update(
    display_name: params[:display_name],
    time_zone: params[:time_zone],
    historic_gpa: params[:historic_gpa],
    historic_units: params[:historic_units],
    api_enabled: params[:api_enabled] == 'true',
    api_listen_all: params[:api_listen_all] == 'true',
    api_key: params[:api_key]
  )

  if params[:host] && params[:cookies] && !params[:cookies].empty?
     $client.save_connection_config(params[:host], params[:cookies])
     # Verify
     whoami = $client.get_who_am_i
     if !whoami || !whoami['Identifier']
       @error = "Updated host/cookies but authentication failed. Please check them."
     end
  end

  redirect '/settings'
end

get '/setup' do
  @host = $client.host
  erb :setup, layout: :layout
end

post '/setup' do
  host = params[:host].strip
  cookies = params[:cookies].strip
  
  # Update actual client instance and persist to config/connection.json
  $client.save_connection_config(host, cookies)
  
  # Verify connection
  whoami = $client.get_who_am_i
  if whoami && whoami['Identifier']
    redirect '/dashboard'
  else
    @error = "Could not authenticate with Brightspace. Please check your host and cookies."
    @host = host
    erb :setup
  end
end

get '/auth/login' do
  host = params[:host] || $client.host
  cookies = BrilliantAuthHelper.fetch_cookies(host)
  
  if cookies
    $client.save_connection_config(host, cookies)
    redirect '/dashboard'
  else
    @error = "Login failed or timed out. Please try again."
    @host = host
    erb :setup
  end
end

before '/course/:id*' do
  redirect '/' unless $client.authenticated?
  
  @user = $client.get_who_am_i
  @course_id = params[:id]

  # Try to load course from normalized table
  @course = Course.find_by(org_unit_id: @course_id.to_s)
  
  # If not in DB, try to fetch info from API to populate name
  if @course.nil? || @course.name.blank? || @course.name.to_s.match?(/^\d+$/)
    course_info = $client.get_org_unit(@course_id)
    
    # Check if we got a valid name from API
    new_name = course_info ? course_info['Name'] : nil
    if new_name && !new_name.to_s.match?(/^\d+$/)
      @course_name = new_name
      
      # Update or create database record
      if @course
        @course.update(name: @course_name, code: course_info['Code'])
      else
        @course = Course.create(org_unit_id: @course_id.to_s, name: @course_name, code: course_info['Code']) rescue nil
      end
    elsif @course && @course.name.present? && !@course.name.to_s.match?(/^\d+$/)
      # Fallback to DB name if API failed but DB has a good name
      @course_name = @course.name
    else
      # Absolute fallback
      @course_name = "Course #{@course_id}"
    end
  else
    @course_name = @course.name
  end
  
  # Fetch TOC from database first
  @toc = build_toc_tree(@course_id)
  
  # Fallback to API/Cache if database is empty for this course
  if @toc['Modules'].empty?
    @toc = $client.get_toc(@course_id)
  end
  
  # Identify the lineage of the current module to keep the sidebar expanded
  current_module_id = params[:module_id] || (request.path.split('/module/')[1] if request.path.include?('/module/'))
  @lineage = find_lineage(@toc['Modules'], current_module_id) if @toc && current_module_id

  # Upcoming Assignments for this course
  @course_upcoming = Assignment.where(course_id: @course_id, completed: false).where("due_date > ? AND due_date <= ?", Time.now, Time.now + 7.days).order(due_date: :asc)
end

get '/' do
  if $client.authenticated?
    redirect '/dashboard'
  else
    erb :login
  end
end

get '/login' do
  redirect $client.auth_url
end

get '/callback' do
  code = params['code']
  if code && $client.exchange_code(code)
    redirect '/dashboard'
  else
    "Authentication Failed. <a href='/'>Retry</a>"
  end
end

get '/dashboard' do
  redirect '/' unless $client.authenticated?
  
  # Trigger proactive sync in background
  $client.sync_all_courses_proactively
  
  @user = $client.get_who_am_i
  # Load courses from normalized table for speed
  @courses_query = Course.all.unscope(:order).order(is_pinned: :desc, last_accessed_at: :desc)
  
  # Fix numeric names on the fly for the dashboard
  @courses_query.each do |c|
    if c.name.to_s.match?(/^\d+$/)
      Thread.new(c.org_unit_id) do |oid|
        ActiveRecord::Base.connection_pool.with_connection do
          info = $client.get_org_unit(oid)
          if info && info['Name'] && !info['Name'].to_s.match?(/^\d+$/)
            Course.find_by(org_unit_id: oid)&.update(name: info['Name'])
          end
        end
      end
    end
  end

  # Fallback if DB is empty
  if @courses_query.empty?
    @courses_raw = $client.get_enrollments || []
    @courses = @courses_raw.map do |c|
      # Defensive mapping to ensure we always have Course-like objects
      Course.new(
        org_unit_id: (c.dig('OrgUnit', 'Id') || 0).to_s,
        name: c.dig('OrgUnit', 'Name') || "Unknown Course",
        code: c.dig('OrgUnit', 'Code'),
        is_pinned: !c['PinDate'].nil?,
        last_accessed_at: (Time.parse(c.dig('Access', 'LastAccessed')) rescue nil),
        semester: $client.extract_semester_from_name(c.dig('OrgUnit', 'Name'))
      )
    end
  else
    @courses = @courses_query
  end

  # --- SEMESTER ANALYTICS ---
  # Defensive mapping: use try(:semester) to handle potential mixed collection/missing columns
  @all_semesters = @courses.map { |c| c.respond_to?(:semester) ? c.semester : nil }.compact.uniq
  @all_semesters = @all_semesters.sort_by { |s| semester_weight(s) }
  
  @latest_semester = @all_semesters.last
  
  # Selection for the Overview box
  @overview_semester = params[:overview_semester] || @user_prefs.default_semester || @latest_semester
  
  # Initialize analytics variables to avoid nil errors in view
  @overall_gpa = 0.0
  @max_potential_gpa = 0.0
  @cumulative_points_earned = 0.0
  @cumulative_points_possible = 0.0
  @semester_grades = []

  # Persistent choice if they select from dropdown
  if params[:overview_semester] && params[:overview_semester] != @user_prefs.default_semester
    @user_prefs.update(default_semester: params[:overview_semester])
  end

  # Filter course list for the "My Course List" section (Existing functionality)
  @semester_filter = params[:semester] || @overview_semester
  
  if @semester_filter && @semester_filter != 'all'
    @display_courses = @courses.select { |c| (c.respond_to?(:semester) ? c.semester : nil) == @semester_filter }
  else
    @display_courses = @courses
  end
  
      # --- CALCULATE ANALYTICS FOR OVERVIEW ---
  if @overview_semester
    @semester_courses = @courses.select { |c| (c.respond_to?(:semester) ? c.semester : nil) == @overview_semester }
    @semester_grades = []
    
    # Target weight for current overview semester
    overview_weight = semester_weight(@overview_semester)
    
    # GPA tracking
    total_weighted_points = 0.0
    total_units_count = 0
    @cumulative_points_earned = 0.0
    @cumulative_points_possible = 0.0
    
    # We include EVERY course up to and including the overview_semester for Cumulative GPA
    @courses.each do |c|
      next unless c.respond_to?(:semester) && c.semester
      c_weight = semester_weight(c.semester)
      next if c_weight > overview_weight
      
      stats = Grade.calculate_weighted_total(c.org_unit_id) rescue nil
      next unless stats
      
      # For the dashboard course cards (only show selected semester)
      # We show the card if there are points or even just a target (for projection)
      if c.semester == @overview_semester
        if stats[:total_points_possible] > 0 || c.target_grade.present?
          @semester_grades << { course: c, stats: stats }
        end
      end

      # For GPA and Cumulative Points calculation
      # STABILIZED GPA LOGIC:
      # 1. For PAST semesters: Use the actual earned score.
      # 2. For CURRENT/OVERVIEW semester: Use the PROJECTED score (earned + target for remaining).
      # This prevents early-semester "dips" and ensures upcoming semesters don't dilute the GPA.
      
      score_to_use = 0.0
      is_future_or_active = (c_weight == overview_weight)
      
      if is_future_or_active
        # Projection: Assume target grade for unearned points
        target = c.target_grade || 93.0
        earned = stats[:total_points_earned] || 0.0
        possible = stats[:total_points_possible] || 0.0
        all_pts = stats[:all_possible_points] || 0.0
        
        remaining = all_pts - possible
        projected_total = earned + (remaining * (target / 100.0))
        
        score_to_use = all_pts > 0 ? (projected_total / all_pts * 100.0) : target
      else
        # Past semester: Use reality
        score_to_use = stats[:score]

        # Fix: If a past semester course has no graded units, skip it to avoid GPA dilution.
        # This handles courses where data is missing or not yet synced.
        if stats[:total_points_possible] == 0
          next
        end
      end

      # Update cumulative stats
      @cumulative_points_earned += stats[:total_points_earned]
      @cumulative_points_possible += stats[:total_points_possible]

      course_gpa_value = Grade.to_gpa(score_to_use)
      course_units = c.units || 3
      total_weighted_points += (course_gpa_value * course_units)
      total_units_count += course_units
    end

    # Historic baseline from preferences
    historic_gpa = @user_prefs.historic_gpa || 0.0
    historic_units = @user_prefs.historic_units || 0
    
    cumulative_points = (historic_gpa * historic_units) + total_weighted_points
    cumulative_units = historic_units + total_units_count
    
    @overall_gpa = cumulative_units > 0 ? (cumulative_points / cumulative_units) : 0.0

    # Max potential (only focusing on the selected overview semester courses)
    total_max_weighted_points = (historic_gpa * historic_units) + total_weighted_points - (@semester_grades.sum { |sg| Grade.to_gpa(sg[:stats][:score]) * (sg[:course].units || 3) })
    @semester_grades.each do |sg|
      course_units = sg[:course].units || 3
      max_course_gpa = Grade.to_gpa(sg[:stats][:max_potential_score])
      total_max_weighted_points += (max_course_gpa * course_units)
    end
    @max_potential_gpa = cumulative_units > 0 ? (total_max_weighted_points / cumulative_units) : 0.0
  end
  # --------------------------

  @recent_notifications = Notification.where(is_read: false).order(date: :desc, id: :desc).limit(10)
  
  # Upcoming Assignments for Dashboard
  @upcoming_assignments = Assignment.where(completed: false).where("due_date > ? AND due_date <= ?", Time.now, Time.now + 7.days).order(due_date: :asc)
  
  @sync_status = $client.sync_status
  
  erb :dashboard
end

post '/assignments/:id/toggle_complete' do
  assignment = Assignment.find(params[:id])
  new_status = !assignment.completed
  assignment.update(completed: new_status, completed_at: (new_status ? Time.now : nil))
  
  ext_id = "assignment_comp_#{assignment.brightspace_id}"
  if new_status
    n = Notification.find_or_initialize_by(external_id: ext_id, course_id: assignment.course_id)
    n.notification_type = "Assignment"
    n.title = "Completed: #{assignment.name}"
    n.body = "You marked this assignment as completed."
    n.date = assignment.completed_at
    n.course_name = assignment.course&.name
    n.semester = assignment.course&.semester
    n.urgency = 1
    n.is_personal = true
    n.url = "/course/#{assignment.course_id}/assignments/#{assignment.brightspace_id}"
    n.is_read = true
    n.save
  else
    Notification.where(external_id: ext_id).destroy_all
  end

  if request.xhr?
    content_type :json
    { status: 'ok', completed: assignment.completed, notification_id: n&.id }.to_json
  else
    redirect back
  end
end

post '/assignments/:id/update_due_date' do
  assignment = Assignment.find(params[:id])
  new_date = params[:due_date]
  if new_date.present?
    # Simple parse, assuming YYYY-MM-DD from a date input
    assignment.update(due_date: Time.parse(new_date))
  else
    assignment.update(due_date: nil)
  end
  
  if request.xhr?
    content_type :json
    { status: 'ok', due_date: assignment.due_date ? assignment.due_date.iso8601 : nil }.to_json
  else
    redirect back
  end
end


# Sync Status Endpoint
get '/sync/status' do
  content_type :json
  $client.sync_status.to_json
end

post '/sync/force' do
  UserPreference.set('force_full_sync', 'true')
  $client.sync_all_courses_proactively
  redirect '/dashboard'
end

# Course Overview / Syllabus
get '/course/:id' do
  @active_tab = 'overview'
  @breadcrumb_trail = [{ title: 'Overview', url: "/course/#{@course_id}" }]
  @overview = $client.get_overview(@course_id)
  @syllabus_info = find_syllabus_items(@toc['Modules']) if @toc
  
  erb :course_detail
end

# Specific Module Details
get '/course/:id/module/:module_id' do
  @module_id = params[:module_id]
  @active_tab = "module_#{@module_id}"
  @module = find_module(@toc['Modules'], @module_id)
  @breadcrumbs = build_breadcrumbs(@toc['Modules'], @module_id) if @toc

  @breadcrumb_trail = (@breadcrumbs || []).map do |crumb|
    { title: crumb[:title], url: "/course/#{@course_id}/module/#{crumb[:id]}" }
  end
  
  erb :module_detail
end

post '/course/:id/module/:module_id/create_tasks' do
  @module_id = params[:module_id]
  selected_indices = params[:tasks] # Array of indices
  
  if selected_indices.nil? || selected_indices.empty?
    flash[:error] = "No tasks selected"
    redirect back
  end

  built_tasks = []
  selected_indices.each do |idx|
    name = params["task_names_#{idx}"]
    type = params["task_types_#{idx}"]
    date_str = params["task_dates_#{idx}"]
    url = params["task_urls_#{idx}"]
    node_module_id = params["task_module_ids_#{idx}"] || @module_id

    # Strip if empty string (placeholder for date input)
    date_str = nil if date_str.blank?

    due_date = date_str.present? ? (Time.parse(date_str) rescue nil) : nil
    # Fallback if parsing failed or was never there
    due_date ||= (Time.now.end_of_week - 1.day).change(hour: 23, min: 59)

    # Determine ext_id using index to keep it unique per module
    ext_id = "syn_#{node_module_id}_#{idx}"
    existing = Assignment.find_by(brightspace_id: ext_id, course_id: @course_id)
    next if existing

    Assignment.create(
      course_id: @course_id,
      brightspace_id: ext_id,
      name: "[#{type}] #{name}",
      due_date: due_date,
      description: "Synthesized from Module: #{params[:module_title] || @module_id}",
      assignment_type: 'synthetic',
      external_url: url,
      synthetic: true
    )
    built_tasks << name
  end

  if built_tasks.any?
    flash[:success] = "Created #{built_tasks.size} tasks in Assignments"
  else
    flash[:error] = "Selected tasks already exist in Assignments"
  end
  
  redirect back
end

# Assignments
get '/course/:id/assignments' do
  @active_tab = 'assignments'
  @breadcrumb_trail = [{ title: 'Assignments', url: "/course/#{@course_id}" }]
  
  # Load real assignments (not synthetic) to check if we need to sync
  real_assignments = Assignment.where(course_id: @course_id, synthetic: false)
  
  if real_assignments.empty?
     # Immediate fallback to API to avoid blank screen
     raw = $client.get_assignments(@course_id)
     sync_data = $client.ensure_array(raw)
     # If we got raw data, trigger the background sync
     Thread.new { ActiveRecord::Base.connection_pool.with_connection { $client.sync_assignments(@course_id, raw) } } if raw
  end

  # Load ALL assignments (real + synthetic) for the view
  @assignments = Assignment.where(course_id: @course_id).order(due_date: :asc)

  erb :assignments
end

# Export Assignments to ICS
get '/course/:id/assignments/export.ics' do
  assignments = $client.get_assignments(@course_id) || []
  
  cal = Icalendar::Calendar.new
  assignments.each do |a|
    next unless a['DueDate']

      event_start = (Time.parse(a['DueDate']) rescue Time.now)
      cal.event do |e|
        e.dtstart     = Icalendar::Values::DateTime.new(event_start.utc, tzid: 'UTC')
        e.dtend       = Icalendar::Values::DateTime.new((event_start + 60*60).utc, tzid: 'UTC')
        e.summary     = "[#{@course_name}] #{a['Name']}"
        e.description = "Assignment Due. Check Brilliant for instructions."
        e.url         = "http://localhost:4567/course/#{@course_id}/assignments/#{a['Id']}"
      end
    end
  
  content_type 'text/calendar'
  attachment "#{@course_name.gsub(/[^0-9a-z]/i, '_')}_Assignments.ics"
  cal.to_ical
end

# Image Proxy to handle Brightspace Auth and Caching
get '/api/proxy/banner' do
  url = params[:url]
  halt 400, "URL required" if url.nil? || url.empty?
  
  # Ensure the URL is fully decoded (it might be encoded in the param)
  url = CGI.unescape(url) if url.include?('%')

  # Create local cache directory if it doesn't exist
  cache_dir = File.expand_path(File.join(File.dirname(__FILE__), 'public', 'banners'))
  FileUtils.mkdir_p(cache_dir)
  # Generate a unique filename for this URL
  file_ext = File.extname(URI.parse(url).path) rescue ".jpg"
  file_ext = ".jpg" if file_ext.empty? # Default to jpg for Brightspace images
  filename = Digest::SHA1.hexdigest(url) + file_ext
  local_path = File.join(cache_dir, filename)

  # Return cached file if it exists
  if File.exist?(local_path)
    return send_file local_path
  end

  # Pass the URL directly to client; client now handles absolute URLs
  response = $client.download_file(url)
  
  if response && response.code == '200'
    begin
      # Save to local cache
      File.open(local_path, 'wb') { |f| f.write(response.body) }
    rescue => e
      puts "[Proxy] Cache write error: #{e.message}"
    end

    content_type response['Content-Type'] || 'image/jpeg'
    response.body
  else
    # Log but don't halt if we want to potentially provide a placeholder later
    puts "[Proxy] Download failed for #{url}: #{response ? response.code : 'NIL'}"
    halt 404, "Image not found"
  end
end

# Enhanced Assignment Details
get '/course/:id/assignments/:assignment_id' do
  @active_tab = 'assignments'
  @assignment_id = params[:assignment_id]
  
  if @assignment_id.start_with?('syn_')
    # Synthetic Assignment Detail
    rec = Assignment.find_by(brightspace_id: @assignment_id, course_id: @course_id)
    puts "[DEBUG] Route: /course/#{@course_id}/assignments/#{@assignment_id}"
    puts "[DEBUG] Found record: #{rec.inspect}"
    halt 404, "Task not found" unless rec
    
    @assignment = {
      'Id' => rec.brightspace_id,
      'Name' => rec.name,
      'DueDate' => rec.due_date&.iso8601,
      'Instructions' => { 'Text' => rec.description },
      'Synthetic' => true,
      'ExternalUrl' => rec.external_url,
      'Completed' => rec.completed
    }
  else
    # Regular Brightspace assignment
    @assignment = $client.get_assignment(@course_id, @assignment_id)
  end
  
  halt 404, "Assignment not found" unless @assignment

  @feedback = $client.get_assignment_feedback(@course_id, @assignment_id) unless @assignment['Synthetic']
  @rubrics = $client.get_assignment_rubrics(@course_id, @assignment_id) unless @assignment['Synthetic']
  @submission_data = $client.get_assignment_submissions(@course_id, @assignment_id) unless @assignment['Synthetic']
  
  sub_group = nil
  if @submission_data.is_a?(Array) && !@submission_data.empty?
      sub_group = @submission_data[0]
      @submissions = sub_group['Submissions']
      if (@feedback.nil? || (@feedback['Feedback']&.empty? rescue true))
          @feedback = sub_group['Feedback']
      end
  end

  # Check for rubrics in the submission data if not found in specific endpoint
  if (@rubrics.nil? || @rubrics.empty?) && sub_group && sub_group['Rubrics']
    @rubrics = sub_group['Rubrics']
  end

  # Fallback to Rubric Definition if no assessment rubric is found
  if (@rubrics.nil? || @rubrics.empty?) && @assignment['Assessment'] && @assignment['Assessment']['Rubrics']
    @rubrics = @assignment['Assessment']['Rubrics']
  end

  if !@assignment['Synthetic'] && (@feedback.nil? || (@feedback['Feedback']&.empty? rescue true))
    # Passing force_refresh: true to ensure we get the latest gradebook data if assignment feedback is missing
    grades = $client.get_grades(@course_id, force_refresh: true)
    @grade_entry = grades.find { |g| g['GradeObjectIdentifier'] == @assignment['GradeItemId'].to_s } if grades && @assignment['GradeItemId']
  end

  @breadcrumb_trail = [
    { title: 'Assignments', url: "/course/#{@course_id}/assignments" },
    { title: @assignment['Name'], url: "/course/#{@course_id}/assignments/#{@assignment_id}" }
  ]

  # Persistence
  @feedback_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:feedback")
  @instructions_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:instructions")
  @submissions_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:submissions")
  @rubric_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:rubric")

  erb :assignment_detail
end

# Quizzes
get '/course/:id/quizzes/:quiz_id' do
  @active_tab = 'assignments' # Group with assignments
  @quiz_id = params[:quiz_id]
  @course_id = params[:id]
  
  # Fetch Quiz Info
  @quiz = $client.do_get("/d2l/api/le/1.40/#{@course_id}/quizzes/#{@quiz_id}")
  halt 404, "Quiz not found" unless @quiz

  @breadcrumb_trail = [
    { title: 'Assignments', url: "/course/#{@course_id}/assignments" },
    { title: @quiz['Name'], url: "/course/#{@course_id}/quizzes/#{@quiz_id}" }
  ]

  # Simple detail view - reuse assignment layout or separate
  erb :quiz_detail
end

get '/course/:id/quizzes' do
  # Redirect to assignments since we display them together there
  redirect "/course/#{params[:id]}/assignments"
end

# Announcements
get '/course/:id/announcements' do
  @active_tab = 'announcements'
  @breadcrumb_trail = [{ title: 'Announcements', url: "/course/#{@course_id}/announcements" }]
  @news = $client.get_news(@course_id)
  erb :announcements
end

post '/course/:id/announcements/:announcement_id/create_task' do
  @course_id = params[:id]
  announcement_id = params[:announcement_id]
  announcement_title = params[:title]
  
  ext_id = "syn_ann_#{announcement_id}"
  existing = Assignment.find_by(brightspace_id: ext_id, course_id: @course_id)
  
  if existing
    flash[:error] = "A task for this announcement already exists."
    redirect back
  end

  # Default due date to end of current week
  due_date = (Time.now.end_of_week - 1.day).change(hour: 23, min: 59)

  Assignment.create(
    course_id: @course_id,
    brightspace_id: ext_id,
    name: "[Announcement] #{announcement_title}",
    due_date: due_date,
    description: "Synthesized from Announcement: #{announcement_title}",
    assignment_type: 'synthetic',
    external_url: "/course/#{@course_id}/announcements",
    synthetic: true
  )

  flash[:success] = "Created task for: #{announcement_title}"
  redirect back
end


# Notifications
get '/notifications' do
  @active_tab = 'notifications'
  @courses = $client.get_enrollments
  @user = $client.get_who_am_i

  # Handle breadcrumb context
  context_id = params[:from_course] || params[:course_id]
  if context_id
    @context_course = Course.find_by(org_unit_id: context_id)
  end
  
  # Trigger a quick background sync for news/grades when viewing notifications
  Thread.new { ActiveRecord::Base.connection_pool.with_connection { $client.sync_notifications(@courses, @user) } }

  query = Notification.all

  # Filter: Course
  if params[:course_id] && !params[:course_id].empty?
    query = query.where(course_id: params[:course_id])
  end
  
  # Filter: Semester
  if params[:semester] && !params[:semester].empty?
    query = query.where(semester: params[:semester])
  end

  # Filter: Urgency
  if params[:urgency] && !params[:urgency].empty?
    query = query.where(urgency: params[:urgency])
  end

  # Filter: Type
  if params[:type] && !params[:type].empty?
    query = query.where(notification_type: params[:type])
  end

  # Filter: Personal
  if params[:personal_only] == 'true'
    query = query.where(is_personal: true)
  end

  # Filter: Read/Unread
  if params[:show_read] == 'true'
    # show all (read + unread), no extra filter
  else
    query = query.where(is_read: false)
  end
  
  # Base relation before sorting/pagination for counting
  @notifications_total = query.count

  # Sort logic
  sort_by = params[:sort] || 'date'
  if sort_by == 'urgency'
    query = query.unscope(:order).order(urgency: :desc, date: :desc, id: :desc)
  else
    query = query.unscope(:order).order(date: :desc, id: :desc)
  end

  # Pagination
  @page = (params[:page] || 1).to_i
  @per_page = 25
  offset = (@page - 1) * @per_page

  @total_pages = (@notifications_total.to_f / @per_page).ceil
  if request.xhr?
    erb :'partials/notifications_list', layout: false
  else
    erb :notifications
  end
end

get '/course/:id/notifications' do
  @active_tab = 'notifications'
  @course_id = params[:id]
  @course = Course.find_by(org_unit_id: @course_id)
  @course_name = @course&.name || "Course #{@course_id}"
  
  # Identify the breadcrumb trail context
  @breadcrumb_trail = [{ title: 'Notifications', url: "/course/#{@course_id}/notifications" }]
  @context_course = @course

  # Trigger background sync for this course news specifically
  $client.sync_notifications([{'OrgUnit' => {'Id' => @course_id, 'Name' => @course_name}}], $client.get_who_am_i)

  query = Notification.where(course_id: @course_id)

  # Apply standard filters
  query = query.where(notification_type: params[:type]) if params[:type] && !params[:type].empty?
  query = query.where(urgency: params[:urgency]) if params[:urgency] && !params[:urgency].empty?
  query = query.where(is_personal: true) if params[:personal_only] == 'true'
  query = query.where(is_read: false) unless params[:show_read] == 'true'

  # Sort, Count, and Paginate
  @notifications_total = query.count
  @page = (params[:page] || 1).to_i
  @per_page = 25
  @total_pages = (@notifications_total.to_f / @per_page).ceil
  
  sort_by = params[:sort] || 'date'
  query = (sort_by == 'urgency') ? query.unscope(:order).order(urgency: :desc, date: :desc, id: :desc) : query.unscope(:order).order(date: :desc, id: :desc)
  
  @notifications = query.offset((@page - 1) * @per_page).limit(@per_page)

  if request.xhr?
    erb :'partials/notifications_list', layout: false
  else
    erb :course_notifications
  end
end

post '/notifications/:id/mark_read' do
  notification = Notification.find(params[:id])
  notification.update(is_read: true)

  # Brightspace Integration: Sync Read Status
  Thread.new do
    ext_id = notification.external_id
    if ext_id.start_with?("news_")
      # Format: news_{course_id}_{item_id}
      parts = ext_id.split('_')
      $client.dismiss_news_item(parts[1], parts[2]) if parts.size >= 3
    elsif ext_id.match?(/^\d+$/)
      # Pure numeric IDs in the LP feed are usually standard notifications
      $client.mark_notification_read(ext_id)
    end
    # Note: Content updates and Grades don't always have a direct "mark as read" 
    # in the API same way News does, but we perform the local state change above.
  end

  if request.xhr?
    content_type :json
    { status: 'ok', id: notification.id, is_read: true }.to_json
  else
    redirect back
  end
end

# Proxy route to mark as read when clicking and then redirect to content
get '/notifications/:id/view' do
  notification = Notification.find(params[:id])
  notification.update(is_read: true)

  # Brightspace Integration sync (background)
  Thread.new do
    ext_id = notification.external_id
    begin
      if ext_id.start_with?("news_")
        parts = ext_id.split('_')
        $client.dismiss_news_item(parts[1], parts[2]) if parts.size >= 3
      elsif ext_id.match?(/^\d+$/)
        $client.mark_notification_read(ext_id)
      end
    rescue => e
      puts "[Brilliant] View/Sync Error: #{e.message}"
    end
  end

  target_url = notification.url
  # Append param to show the "Keep Unread" bar if it's a local internal link
  if target_url.start_with?('/')
    separator = target_url.include?('?') ? '&' : '?'
    target_url += "#{separator}from_notification=#{notification.id}"
  end
  
  redirect target_url
end

post '/notifications/:id/mark_unread' do
  notification = Notification.find(params[:id])
  notification.update(is_read: false)

  if request.xhr?
    content_type :json
    { status: 'ok', id: notification.id, is_read: false }.to_json
  else
    redirect back
  end
end

post '/notifications/mark_all_read' do
  # Identify the IDs we are about to mark read across Brightspace
  unread_notifications = Notification.where(is_read: false).select(:external_id, :course_id)
  
  Notification.update_all(is_read: true)

  # Sync all dismissals to Brightspace in background
  Thread.new do
    unread_notifications.each do |n|
      ext_id = n.external_id
      begin
        if ext_id.start_with?("news_")
          parts = ext_id.split('_')
          $client.dismiss_news_item(parts[1], parts[2]) if parts.size >= 3
        elsif ext_id.match?(/^\d+$/)
          $client.mark_notification_read(ext_id)
        end
      rescue => e
        puts "[Brilliant] Bulk background sync error: #{e.message}"
      end
      # Small sleep to prevent rate limiting D2L
      sleep 0.1
    end
  end

  redirect '/notifications'
end

post '/notifications/clear' do
  Notification.delete_all
  redirect '/notifications'
end

post '/notifications/refresh_cache' do
  ApiCache.delete_all
  Notification.delete_all
  redirect '/notifications'
end

get '/debug/notifications' do
  content_type :json
  order = params[:order] == 'asc' ? :asc : :desc
  {
    total: Notification.count,
    first_item: Notification.order(date: order).first,
    last_item: Notification.order(date: order).last,
    sample: Notification.order(date: order).limit(5).map { |n| { id: n.id, date: n.date, title: n.title, semester: n.semester, is_read: n.is_read } },
    distinct_semesters: Notification.pluck(:semester).uniq
  }.to_json
end

# --- Centralized Error Handling ---
not_found do
  status 404
  @error_title = "404 - Not Found"
  @error_message = "The page you are looking for does not exist or has been moved."
  erb :error
end

error do
  @error = env['sinatra.error']
  status 500
  @error_title = "500 - Server Error"
  @error_message = "An unexpected error occurred while processing your request."
  puts "[Brilliant Error] #{@error.message}"
  puts @error.backtrace.first(10).join("\n")
  erb :error
end

# Grades
get '/course/:id/grades' do
  @active_tab = 'grades'
  @course ||= Course.find_by(org_unit_id: params[:id].to_s)
  @breadcrumb_trail = [{ title: 'Grades', url: "/course/#{@course_id}/grades" }]
  
  # Trigger refresh and sync
  @grades_raw = $client.get_grades(@course_id) || []
  Thread.new { $client.sync_grades(@course_id, @grades_raw) }
  
  # Use DB for display
  # Sort by due_date ASC. Items without due dates will appear at the bottom (nulls last)
  @grades = Grade.where(course_id: @course_id).order(Arel.sql("due_date ASC NULLS LAST"), name: :asc)
  
  # Fallback to raw if DB still empty
  @grades = @grades_raw if @grades.empty?

  # Calculate Grade Stats (Analytics)
  @grade_stats = calculate_grade_stats(@course_id)
  
  erb :grades
end

post '/course/:id/update_units' do
  @course = Course.find_by(org_unit_id: params[:id])
  @course.update(units: params[:units]) if @course
  redirect back
end

post '/course/:id/update_target_grade' do
  @course = Course.find_by(org_unit_id: params[:id])
  @course.update(target_grade: params[:target_grade]) if @course
  redirect back
end

# Discussions
get '/course/:id/discussions' do
  @active_tab = 'discussions'
  
  # Always trigger a fast background refresh of metadata (counts, etc)
  # This keeps the Dashboard and Discussion tables fresh
  Thread.new(@course_id) do |cid|
    ActiveRecord::Base.connection_pool.with_connection do
      forums_raw = $client.get_discussions(cid) || []
      $client.sync_discussions(cid, forums_raw)
      
      # Deep sync: for each topic, trigger a post sync to verify counts
      DiscussionTopic.where(course_id: cid).find_each do |topic|
        posts_data = $client.get_topic_posts(cid, topic.forum_id, topic.brightspace_id)
        posts = posts_data.is_a?(Hash) ? (posts_data['Items'] || []) : posts_data
        $client.sync_topic_posts(cid, topic.forum_id, topic.brightspace_id, posts) if posts
      end
    end
  end

  @forums = DiscussionForum.where(course_id: @course_id).order(name: :asc)
  @topics = DiscussionTopic.where(course_id: @course_id).order(sort_order: :asc)

  if @topics.empty? || params[:force_refresh] == 'true'
    # Immediate fallback to raw data if DB is empty to avoid blank screen
    @forums_raw = $client.get_discussions(@course_id) || []
    @forums = @forums_raw # Populate @forums for the view too
    @topics = @forums_raw.flat_map do |f|
      topics_data = $client.get_discussion_topics(@course_id, f['ForumId'], force_refresh: params[:force_refresh] == 'true')
      # Defensive check: ensure topics_data is a hash/array before mapping
      items = if topics_data.is_a?(Hash)
                topics_data['Items'] || []
              elsif topics_data.is_a?(Array)
                topics_data
              else
                []
              end
      
      items.each { |t| t['ForumId'] = f['ForumId'] if t.is_a?(Hash) }
      items
    end
  end
  
  @breadcrumb_trail = [{ title: 'Discussions', url: "/course/#{@course_id}/discussions" }]
  erb :discussions
end

# Discussion Topics
get '/course/:id/discussions/:forum_id/topics' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  force_refresh = params[:force_refresh] == 'true'
  
  @forum = DiscussionForum.find_by(brightspace_id: @forum_id, course_id: @course_id)
  # Fallback
  @forum ||= $client.get_discussion_forum(@course_id, @forum_id)

  @topics = DiscussionTopic.where(forum_id: @forum_id, course_id: @course_id)
  @topics = @topics.order(sort_order: :asc) if @topics.any?

  # Sort by database or refresh if requested/empty
  if @topics.empty? || force_refresh
    topics_raw = $client.get_discussion_topics(@course_id, @forum_id, force_refresh: force_refresh)
    @topics_data = topics_raw.is_a?(Hash) ? (topics_raw['Items'] || []) : (topics_raw || [])
    Thread.new { $client.sync_discussion_topics(@course_id, @forum_id, @topics_data) }
    @topics = @topics_data if @topics.empty?
  end

  @breadcrumb_trail = [
    { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
    { title: @forum.is_a?(DiscussionForum) ? @forum.name : @forum['Name'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics" }
  ]
  erb :discussion_topics
end

# Discussion Threads
get '/course/:id/discussions/:forum_id/topics/:topic_id' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @topic_id = params[:topic_id]
  
  @forum = DiscussionForum.find_by(brightspace_id: @forum_id, course_id: @course_id) || $client.get_discussion_forum(@course_id, @forum_id)
  @topic = DiscussionTopic.find_by(brightspace_id: @topic_id, forum_id: @forum_id) || $client.get_discussion_topic(@course_id, @forum_id, @topic_id)
  @evaluation = $client.get_discussion_evaluation(@course_id, @forum_id, @topic_id)
  
  # FALLBACK: If API evaluation is hidden (404/nil), search Grades for a matching name
  if @evaluation.nil?
    topic_name = @topic.is_a?(DiscussionTopic) ? @topic.name : @topic['Name']
    # Try fuzzy match in DB
    grade_match = Grade.where(course_id: @course_id).where("name LIKE ?", "%#{topic_name}%").first
    if grade_match && grade_match.comments.present?
      @evaluation = {
        'Feedback' => { 'Html' => grade_match.comments },
        'Score' => grade_match.displayed_grade
      }
    end
  end

  force_refresh = params[:force_refresh] == 'true'
  
  @breadcrumb_trail = [
    { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
    { title: @topic.is_a?(DiscussionTopic) ? @topic.name : @topic['Name'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}" }
  ]
  
  # Fetch and sync threads
  # Refactor: Fetch all posts for the topic in one go. 
  # This is much faster and bypasses 404 issues on the /threads/ collection endpoint.
  posts_data_raw = $client.get_topic_posts(@course_id, @forum_id, @topic_id, force_refresh: force_refresh)
  all_posts = posts_data_raw.is_a?(Hash) ? (posts_data_raw['Items'] || []) : (posts_data_raw || [])

  # Determine if manual post exists for collapse logic
  target_name = @user_prefs.display_name.to_s.strip.downcase
  
  # Check local DB first for participation (most reliable)
  participated = DiscussionPost.where(topic_id: @topic_id.to_s)
                               .where("lower(author_name) = ?", target_name)
                               .exists?
  
  puts "DEBUG: @@topic_id=#{@topic_id} participated=#{participated}"
  
  if !participated
    user_post = all_posts.find do |p| 
      author = p['PostingUserDisplayName'].to_s.strip.downcase
      author == target_name
    end
    participated = !user_post.nil?
    puts "DEBUG: Memo path user_post found=#{!user_post.nil?}"
  end

  # Persistence + Participation
  @instructions_collapsed = @user_prefs.topic_collapsed?("#{@topic_id}:instructions") || participated
  @feedback_collapsed = @user_prefs.topic_collapsed?("#{@topic_id}:feedback")

  # FALLBACK: If posts are empty, try fetching threads directly.
  # This handles topics that exist but have no posts yet (or Synthesis fails)
  if all_posts.empty?
    threads_raw = $client.get_discussion_threads(@course_id, @forum_id, @topic_id, force_refresh: force_refresh)
    threads_list = threads_raw.is_a?(Hash) ? (threads_raw['Items'] || []) : (threads_raw || [])
    
    @threads_with_posts = threads_list.map do |t|
      thread = {
        'ThreadId' => t['ThreadId'],
        'Subject' => t['Subject'] || t['Title'] || "No Subject",
        'Title' => t['Subject'] || t['Title'] || "No Subject",
        'PostingUserDisplayName' => t['PostingUserDisplayName'],
        'LastModified' => t['LastModified'] || t['DatePosted'],
        'IsPinned' => t['IsPinned'] || false,
        'ReplyCount' => t['ReplyCount'] || 0
      }
      
      # We have no posts to show, but we can show the thread structure
      {
        thread: thread,
        post_tree: [] # No posts available to synthesize a tree
      }
    end
    
    return erb :discussion_threads
  end

  # NORMAL PATH: Use posts to synthesize threads
  Thread.new(@course_id, @forum_id, @topic_id, all_posts) do |cid, fid, tid, posts|
    ActiveRecord::Base.connection_pool.with_connection do
      $client.sync_topic_posts(cid, fid, tid, posts)
    end
  end

  # Transition to using normalized data if available, but fallback to raw for this request
  # to ensure immediate feedback.
  # Group posts by ThreadId
  threads_groups = all_posts.group_by { |p| p['ThreadId'] }
  
  @threads_with_posts = threads_groups.map do |thread_id, posts|
    # Find the root post or synthesize thread info
    root_post = posts.find { |p| p['ParentPostId'].nil? } || posts.min_by { |p| p['DatePosted'] }
    
    # Create a thread object that fits the existing view expectations
    # We use the raw hash here because we already have it in memory from the API
    thread = {
      'ThreadId' => thread_id,
      'Subject' => root_post['Subject'] || "No Subject",
      'Title' => root_post['Subject'] || "No Subject",
      'PostingUserDisplayName' => root_post['PostingUserDisplayName'],
      'LastModified' => root_post['DatePosted'] || root_post['LastModified'],
      'IsPinned' => root_post['IsPinned'] || false,
      'ReplyCount' => posts.size - 1
    }

    # Background sync
    Thread.new(thread, @topic_id, @course_id) { |t, tid, cid| $client.sync_discussion_thread(cid, tid, t) }

    {
      thread: thread,
      post_tree: build_post_tree(posts) # build_post_tree already handles ParentPostId nil roots
    }
  end.sort_by { |item| item[:thread]['IsPinned'] ? 0 : 1 }

  erb :discussion_threads
end

post '/course/:id/discussions/toggle_collapse' do
  topic_id = params[:topic_id]
  section = params[:section]
  key = (section && !section.empty?) ? "#{topic_id}:#{section}" : topic_id
  
  @user_prefs.toggle_topic_collapse(key)
  
  if request.xhr?
    content_type :json
    { status: 'ok', collapsed: @user_prefs.topic_collapsed?(key) }.to_json
  else
    redirect back
  end
end

# Discussion Posts
get '/course/:id/discussions/:forum_id/topics/:topic_id/threads/:thread_id' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @topic_id = params[:topic_id]
  @thread_id = params[:thread_id]
  
  @forum = $client.get_discussion_forum(@course_id, @forum_id)
  @topic = $client.get_discussion_topic(@course_id, @forum_id, @topic_id)
  @thread = $client.get_discussion_thread(@course_id, @forum_id, @topic_id, @thread_id)
  @breadcrumb_trail = [
    { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
    { title: @topic['Name'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}" },
    { title: @thread['Title'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}/threads/#{@thread_id}" }
  ]
  @posts_data = $client.get_thread_posts(@course_id, @forum_id, @topic_id, @thread_id)
  @posts = @posts_data.is_a?(Hash) ? (@posts_data['Items'] || []) : (@posts_data || [])

  erb :discussion_posts
end

# Topic Specific Download Route
# Overview Specific Download Route
get '/course/:id/overview/download' do
  course_id = params[:id]
  api_path = "/d2l/api/le/1.40/#{course_id}/overview/attachment"
  http_resp = $client.download_file(api_path)
  
  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type'] || 'application/octet-stream'
    disposition = http_resp['Content-Disposition']
    if disposition && disposition =~ /filename\*?="?([^";]+)"?/
      headers["Content-Disposition"] = disposition
    else
      headers["Content-Disposition"] = "attachment; filename=\"syllabus_#{course_id}.pdf\""
    end
    http_resp.body
  else
    status_code = http_resp ? http_resp.code : 'No Response'
    "Download failed: Status #{status_code} (overview attachment)."
  end
end


get '/course/:id/topic/:topic_id/download' do
  course_id = params[:id]
  topic_id = params[:topic_id]
  
  api_path = "/d2l/api/le/1.40/#{course_id}/content/topics/#{topic_id}/file"
  http_resp = $client.download_file(api_path)
  
  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type'] || 'application/octet-stream'
    disposition = http_resp['Content-Disposition']
    if disposition && disposition =~ /filename\*?="?([^";]+)"?/
      headers["Content-Disposition"] = disposition
    else
      headers["Content-Disposition"] = "attachment; filename=\"topic_#{topic_id}_file\""
    end
    http_resp.body
  else
    status_code = http_resp ? http_resp.code : 'No Response'
    "Download failed: Status #{status_code} (trying to fetch topic #{topic_id} file)."
  end
end


# Generic Download Route
get '/course/:id/download' do
  path = params[:path]
  name = params[:name] || "download"
  course_id = params[:id]
  
  path = "/#{path}" unless path.start_with?('/')

  # Intelligent persistence check
  # Try to find a local version of this file if it was already fetched
  # Path looks like: /d2l/api/le/1.40/248383/dropbox/folders/1445612/attachments/24031641
  file_id = path.split('/').last
  if file_id && !file_id.empty?
    local_dir = File.join(settings.public_folder, 'attachments', course_id.to_s)
    if File.exist?(local_dir)
      local_file = Dir.glob(File.join(local_dir, "#{file_id}_*")).first
      if local_file && File.exist?(local_file)
        return send_file local_file, disposition: 'attachment', filename: name
      end
    end
  end

  http_resp = $client.download_file(path)
  
  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type'] || 'application/octet-stream'
    safe_name = name.gsub(/[^0-9A-Za-z.\- ]/, '_')
    headers["Content-Disposition"] = "attachment; filename=\"#{safe_name}\""
    http_resp.body
  else
    status_code = http_resp ? http_resp.code : 'No Response'
    "Download failed: Status #{status_code}."
  end
end

# Search
get '/course/:id/search' do
  @query = params[:q]
  @active_tab = 'search'
  @results = search_toc(@toc['Modules'], @query) if @toc && @query
  erb :search
end

# Download All
# Download Module All
get '/course/:id/module/:module_id/download_all' do
  mod = find_module(@toc['Modules'], params[:module_id])
  if mod.nil?
    return "Module not found."
  end

  safe_title = mod['Title'].gsub(/[^0-9a-z]/i, '_')
  files = collect_all_files(@course_id, mod, safe_title)
  
  if files.empty?
    return "No downloadable files found in this module."
  end

  filename = "Brilliant-#{@course_id}-#{safe_title}-#{Time.now.strftime('%Y%m%d')}.zip"
  job = DownloadJob.create(@course_id, files, $client, download_filename: filename)
  redirect "/job/#{job.id}"
end

get '/course/:id/download_all' do
  files = collect_everything(@course_id, $client, @toc)
  if files.empty?
    return "No downloadable files found in this course."
  end

  filename = "Brilliant-#{@course_id}-#{Time.now.strftime('%Y%m%d')}.zip"
  job = DownloadJob.create(@course_id, files, $client, download_filename: filename)
  redirect "/job/#{job.id}"
end

# Job Status
get '/job/:id' do
  @job = DownloadJob.find(params[:id])
  erb :job_status
end

get '/job/:id/status' do
  job = DownloadJob.find(params[:id])
  if job
    content_type :json
    { 
      status: job.status, 
      progress: job.progress,
      completed: job.completed_files,
      total: job.total_files
    }.to_json
  else
    status 404
  end
end

get '/job/:id/download' do
  job = DownloadJob.find(params[:id])
  if job && job.status == :completed && File.exist?(job.zip_path)
    send_file job.zip_path, :type => 'application/zip', :disposition => 'attachment', :filename => job.download_filename
  else
    "Not ready."
  end
end

# ==========================================
# External REST API Routes
# ==========================================

# Use a before filter for all API routes to handle auth and content-type
before '/api/v1/*' do
  content_type :json
  @user_prefs = UserPreference.current
  validate_api_access!
end

# --- Course Discovery ---
get '/api/v1/courses' do
  # Trigger intelligent background sync
  $client.sync_all_courses_proactively
  
  courses = Course.all.order(is_pinned: :desc, last_accessed_at: :desc)
  courses.to_json
end

get '/api/v1/courses/:id' do
  course = Course.find_by(org_unit_id: params[:id])
  halt 404, { error: "Course not found" }.to_json unless course
  course.to_json
end

# --- Content ---
get '/api/v1/courses/:id/modules' do
  modules = ContentModule.where(course_id: params[:id]).order(sort_order: :asc)
  modules.to_json
end

get '/api/v1/courses/:id/assignments' do
  assignments = Assignment.where(course_id: params[:id]).order(due_date: :asc)
  assignments.to_json
end

# --- Progress & Performance ---
get '/api/v1/courses/:id/grades' do
  # Only fetch from OA if not in degraded mode
  unless $client.degraded_mode
    grades_raw = $client.get_grades(params[:id])
    $client.sync_grades(params[:id], grades_raw) if grades_raw.is_a?(Array)
  end
  
  grades = Grade.where(course_id: params[:id]).order(Arel.sql("due_date ASC NULLS LAST"))
  grades.to_json
end

get '/api/v1/courses/:id/stats' do
  stats = calculate_grade_stats(params[:id])
  stats.to_json
end

# --- Notifications ---
get '/api/v1/notifications' do
  query = Notification.all
  
  query = query.where(course_id: params[:course_id]) if params[:course_id].present?
  query = query.where(semester: params[:semester]) if params[:semester].present?
  query = query.where(urgency: params[:urgency]) if params[:urgency].present?
  query = query.where(is_personal: true) if params[:is_personal] == 'true'
  
  unless params[:include_read] == 'true' || params[:show_read] == 'true'
    query = query.where(is_read: false)
  end
  
  limit = (params[:limit] || 50).to_i
  query.order(date: :desc).limit(limit).to_json
end

# --- Synthetic Tasks CRUD ---
get '/api/v1/synthetic_tasks' do
  query = Assignment.where(assignment_type: 'synthetic')
  query = query.where(course_id: params[:course_id]) if params[:course_id].present?
  query.to_json
end

post '/api/v1/synthetic_tasks' do
  payload = JSON.parse(request.body.read) rescue {}
  
  due_date = nil
  if payload['due_date'].present?
    due_date = Time.parse(payload['due_date'].to_s) rescue nil
  end
  due_date ||= (Time.now.end_of_week - 1.day).change(hour: 23, min: 59)

  task = Assignment.create(
    course_id: payload['course_id'],
    brightspace_id: payload['id'] || "syn_#{SecureRandom.hex(4)}",
    name: payload['name'],
    due_date: due_date,
    description: payload['description'],
    assignment_type: 'synthetic',
    synthetic: true
  )
  
  if task.persisted?
    task.to_json
  else
    halt 422, { errors: task.errors.full_messages }.to_json
  end
end

patch '/api/v1/synthetic_tasks/:id' do
  task = Assignment.find_by(brightspace_id: params[:id], assignment_type: 'synthetic')
  halt 404, { error: "Task not found" }.to_json unless task

# Dedicated route for internal synthetic task creation (blocks highlighting)
post '/course/:id/synthetic_tasks' do
  content_type :json
  payload = JSON.parse(request.body.read) rescue {}
  
  due_date = nil
  if payload['due_date'].present?
    due_date = Time.parse(payload['due_date'].to_s) rescue nil
  end
  due_date ||= (Time.now.end_of_week - 1.day).change(hour: 23, min: 59)

  task = Assignment.create(
    course_id: params[:id],
    brightspace_id: "syn_man_#{SecureRandom.hex(4)}",
    name: payload['name'],
    due_date: due_date,
    description: payload['description'],
    assignment_type: 'synthetic',
    synthetic: true
  )
  
  if task.persisted?
    task.to_json
  else
    status 422
    { errors: task.errors.full_messages }.to_json
  end
end

  
  payload = JSON.parse(request.body.read) rescue {}
  if task.update(payload.slice('name', 'due_date', 'description', 'completed'))
    task.to_json
  else
    halt 422, { errors: task.errors.full_messages }.to_json
  end
end

delete '/api/v1/synthetic_tasks/:id' do
  task = Assignment.find_by(brightspace_id: params[:id], assignment_type: 'synthetic')
  halt 404, { error: "Task not found" }.to_json unless task
  task.destroy
  status 204
end

# --- Settings ---
get '/api/v1/preferences' do
  @user_prefs.to_json(except: [:api_key, :brightspace_cookie])
end

patch '/api/v1/preferences' do
  payload = JSON.parse(request.body.read) rescue {}
  
  # Whitelist updatable fields
  updatable = payload.slice('display_name', 'time_zone', 'historic_gpa', 'historic_units', 'default_semester')
  
  if @user_prefs.update(updatable)
    @user_prefs.to_json(except: [:api_key, :brightspace_cookie])
  else
    halt 422, { errors: @user_prefs.errors.full_messages }.to_json
  end
end

# --- System & Auth State ---
get '/api/v1/status' do
  {
    authenticated: $client.authenticated?,
    degraded_mode: $client.degraded_mode,
    host: $client.host,
    sync_status: $client.sync_status
  }.to_json
end

# ==========================================
# Model Context Protocol (MCP) Implementation
# ==========================================

set :mcp_connections, {}

get '/api/v1/mcp/sse' do
  validate_api_access!
  
  content_type 'text/event-stream'
  cache_control :no_cache
  headers(
    'Connection' => 'keep-alive',
    'X-Accel-Buffering' => 'no'
  )
  
  stream(:keep_open) do |out|
    session_id = SecureRandom.uuid
    settings.mcp_connections[session_id] = out
    
    # Metadata for the client to know where to POST messages
    out << "event: endpoint\n"
    out << "data: /api/v1/mcp/messages?session_id=#{session_id}\n\n"
    
    out.callback do
      settings.mcp_connections.delete(session_id)
    end
  end
end

post '/api/v1/mcp/messages' do
  validate_api_access!
  
  session_id = params[:session_id]
  out = settings.mcp_connections[session_id]
  
  # Just respond with 200 if it's a direct POST without a session (unlikely for MCP SSE)
  # but if session exists, we use it.
  
  request_payload = JSON.parse(request.body.read) rescue nil
  halt 400, { error: "Invalid JSON-RPC request" }.to_json unless request_payload
  
  response_payload = handle_mcp_request(request_payload)
  
  if response_payload && out
    out << "event: message\n"
    out << "data: #{response_payload.to_json}\n\n"
    status 202
  elsif response_payload
    # Fallback to direct response if no SSE stream (standard HTTP)
    content_type :json
    response_payload.to_json
  else
    status 204
  end
end

helpers do
  def handle_mcp_request(json)
    id = json['id']
    method = json['method']
    params = json['params'] || {}

    case method
    when 'initialize'
      { jsonrpc: "2.0", id: id, result: { protocolVersion: "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "Brilliant-MCP", version: "1.4.3" } } }
    when 'tools/list'
      { jsonrpc: "2.0", id: id, result: { tools: [
        { 
          name: "list_courses", 
          description: "List enrolled courses with advanced filtering", 
          inputSchema: { 
            type: "object", 
            properties: { 
              year: { type: "string", description: "Filter by year (e.g. 2026)" },
              season: { type: "string", description: "Filter by season (e.g. Spring, Fall, Summer)" },
              department_prefix: { type: "string", description: "Filter by department code (e.g. SWO, ECO)" },
              title: { type: "string", description: "Fuzzy match on course title" },
              is_pinned: { type: "boolean", description: "Filter by pinned/favorite status" },
              semester: { type: "string", description: "Exact match for semester string (e.g. '2026 Spring')" }
            } 
          } 
        },
        { 
          name: "get_course_grades", 
          description: "Get grade entries for a specific course", 
          inputSchema: { 
            type: "object", 
            properties: { 
              course_id: { type: "string", description: "Course OrgUnitId" } 
            }, 
            required: ["course_id"] 
          } 
        },
        { 
          name: "get_notifications", 
          description: "Get notifications across all courses with UI-consistent filtering", 
          inputSchema: { 
            type: "object", 
            properties: { 
              course_id: { type: "string", description: "Filter by Course OrgUnitId" },
              semester: { type: "string", description: "Filter by semester (e.g. '2026 Spring')" },
              urgency: { type: "integer", description: "Filter by urgency level (1-5)" },
              is_personal: { type: "boolean", description: "Filter for personal/direct notifications" },
              show_read: { type: "boolean", description: "Set to true to include read notifications (default: false)" },
              limit: { type: "integer", default: 10, description: "Maximum number of notifications to return" }
            } 
          } 
        },
        { 
          name: "get_course_assignments", 
          description: "Get assignments and due dates for a course", 
          inputSchema: { 
            type: "object", 
            properties: { 
              course_id: { type: "string" } 
            }, 
            required: ["course_id"] 
          } 
        },
        {
          name: "list_synthetic_tasks",
          description: "List custom/synthetic tasks created for a course",
          inputSchema: {
            type: "object",
            properties: {
              course_id: { type: "string", description: "Filter by course ID" }
            }
          }
        },
        {
          name: "create_synthetic_task",
          description: "Create a new custom/synthetic assignment task",
          inputSchema: {
            type: "object",
            properties: {
              course_id: { type: "string", description: "Target course ID" },
              name: { type: "string", description: "Task name" },
              due_date: { type: "string", description: "ISO 8601 date string" },
              description: { type: "string", description: "Task notes/instructions" }
            },
            required: ["course_id", "name"]
          }
        },
        {
          name: "update_synthetic_task",
          description: "Update an existing synthetic task",
          inputSchema: {
            type: "object",
            properties: {
              id: { type: "string", description: "The 'syn_...' ID of the task" },
              name: { type: "string" },
              due_date: { type: "string" },
              description: { type: "string" },
              completed: { type: "boolean" }
            },
            required: ["id"]
          }
        },
        {
          name: "delete_synthetic_task",
          description: "Remove a synthetic task",
          inputSchema: {
            type: "object",
            properties: {
              id: { type: "string", description: "The 'syn_...' ID to delete" }
            },
            required: ["id"]
          }
        }
      ] } }
    when 'tools/call'
      result = call_mcp_tool(params['name'], params['arguments'] || {})
      { jsonrpc: "2.0", id: id, result: result }
    when 'notifications/initialized'
      nil
    else
      { jsonrpc: "2.0", id: id, error: { code: -32601, message: "Method not found: #{method}" } }
    end
  end

  def call_mcp_tool(name, args)
    case name
    when 'list_courses'
      query = Course.all
      
      query = query.where("semester LIKE ?", "%#{args['year']}%") if args['year']
      query = query.where("semester LIKE ?", "%#{args['season']}%") if args['season']
      query = query.where(semester: args['semester']) if args['semester']
      query = query.where("name LIKE ?", "%#{args['department_prefix']} %") if args['department_prefix']
      query = query.where("name LIKE ?", "%#{args['title']}%") if args['title']
      query = query.where(is_pinned: (args['is_pinned'] == true)) if args.has_key?('is_pinned')

      courses = query.order(is_pinned: :desc, last_accessed_at: :desc)
      { content: [{ type: "text", text: courses.to_json }] }

    when 'get_course_grades'
      grades = Grade.where(course_id: args['course_id']).order(Arel.sql("due_date ASC NULLS LAST"))
      { content: [{ type: "text", text: grades.to_json }] }

    when 'get_notifications'
      query = Notification.all
      
      query = query.where(course_id: args['course_id']) if args['course_id']
      query = query.where(semester: args['semester']) if args['semester']
      query = query.where(urgency: args['urgency']) if args['urgency']
      query = query.where(is_personal: true) if args['is_personal']
      
      if args['show_read'] != true
        query = query.where(is_read: false)
      end

      limit = args['limit'] || 10
      items = query.order(date: :desc).limit(limit)
      { content: [{ type: "text", text: items.to_json }] }

    when 'get_course_assignments'
      assignments = Assignment.where(course_id: args['course_id']).order(due_date: :asc)
      { content: [{ type: "text", text: assignments.to_json }] }

    when 'list_synthetic_tasks'
      query = Assignment.where(assignment_type: 'synthetic')
      query = query.where(course_id: args['course_id']) if args['course_id']
      { content: [{ type: "text", text: query.to_json }] }

    when 'create_synthetic_task'
      task = Assignment.create(
        course_id: args['course_id'],
        brightspace_id: "syn_#{SecureRandom.hex(4)}",
        name: args['name'],
        due_date: args['due_date'].present? ? (Time.parse(args['due_date'].to_s) rescue nil) : nil,
        description: args['description'],
        assignment_type: 'synthetic'
      )
      if task.due_date.nil?
        task.update(due_date: (Time.now.end_of_week - 1.day).change(hour: 23, min: 59))
      end
      { content: [{ type: "text", text: task.to_json }] }

    when 'update_synthetic_task'
      task = Assignment.find_by(brightspace_id: args['id'], assignment_type: 'synthetic')
      if task
        task.update(args.slice('name', 'due_date', 'description', 'completed'))
        { content: [{ type: "text", text: task.to_json }] }
      else
        { isError: true, content: [{ type: "text", text: "Task not found" }] }
      end

    when 'delete_synthetic_task'
      task = Assignment.find_by(brightspace_id: args['id'], assignment_type: 'synthetic')
      if task
        task.destroy
        { content: [{ type: "text", text: "Task deleted" }] }
      else
        { isError: true, content: [{ type: "text", text: "Task not found" }] }
      end

    else
      { isError: true, content: [{ type: "text", text: "Tool not found: #{name}" }] }
    end
  end
end

# Transition to Start Server
if __FILE__ == $0
  # Print Startup Info
  port = settings.port
  bind = settings.bind == '0.0.0.0' ? 'localhost' : settings.bind
  
  puts "\n" + "="*60
  puts " [Brilliant] Server Instance: http://#{bind}:#{port}"
  puts " [Brilliant] API Base URL:    http://#{bind}:#{port}/api/v1"
  puts " [Brilliant] API Docs:        http://#{bind}:#{port}/docs"
  puts "="*60 + "\n"

  # Handle Headless Mode
  if ARGV.include?('--headless')
    puts "[Brilliant] Running in HEADLESS mode (No Electron)"
    # We use Sinatra's built-in run!
    Sinatra::Application.run!
  else
    # In standard mode, Sinatra is usually started by the sidecar manager, 
    # but we'll maintain compatibility here.
    Sinatra::Application.run!
  end
end
