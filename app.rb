require 'bundler/setup'
require 'fileutils'
require 'uri'
require 'cgi'

# Speed up boot using bootsnap
require 'bootsnap'
cache_dir = ENV['BOOTSNAP_CACHE_DIR'] || 'tmp/bootsnap'
FileUtils.mkdir_p(cache_dir)
Bootsnap.setup(
  cache_dir:            cache_dir, 
  development_mode:     ENV['RACK_ENV'] == 'development' || ENV['BRILLIANT_ENV'] == 'electron',
  load_path_cache:      true,
  compile_cache_iseq:   true,
  compile_cache_yaml:   true
)

Bundler.require(:default)

require 'sinatra'
require 'sinatra/activerecord'
require 'active_support/time'
require 'time'
require 'zip'
require 'tempfile'
require 'icalendar'
require 'rack-flash'

require_relative 'lib/brightspace/client'
require_relative 'lib/brightspace/auth_helper'
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

# Basic Configuration
set :port, 4567
set :views, File.join(File.dirname(__FILE__), 'views')

# PID Management for Electron Sidecar
if ENV['BRILLIANT_DATA_DIR']
  pid_path = File.join(ENV['BRILLIANT_DATA_DIR'], 'ruby_sidecar.pid')
  File.write(pid_path, Process.pid.to_s)
  at_exit { File.delete(pid_path) if File.exist?(pid_path) }
end

# Reduce logging noise in production/Electron context
if ENV['RACK_ENV'] == 'production' || defined?(Electron)
  ActiveRecord::Base.logger.level = Logger::INFO
end

# Session and Middleware Setup (Must be before routes and error blocks)
enable :sessions
use Rack::Flash

# Global Error Handling for API Auth
error BrightspaceClient::AuthenticationError do
  msg = env['sinatra.error'].message
  status_code = env['sinatra.error'].respond_to?(:status_code) ? env['sinatra.error'].status_code : 401
  
  # Accessing env['x-rack.flash'] directly in error blocks is safer
  flash_obj = env['x-rack.flash'] || env['rack.flash']
  flash_obj[:error] = "<strong>Session Error (#{status_code})</strong>: Your Brightspace session has expired. <a href='/auth/login' class='has-text-weight-bold' style='text-decoration: underline'>Click here to Magic Login</a>" if flash_obj
  
  redirect '/dashboard'
end

# Runtime Database Initialization
configure :development, :production do
  begin
    puts "Verifying database schema..."
    
    # Increase connection pool size to handle background sync threads
    db_config = {}
    
    # If using absolute path in DATABASE_URL, ensure parent directory exists
    if ENV['DATABASE_URL'] && ENV['DATABASE_URL'].start_with?('sqlite3://')
      # Correctly parse URI and handle %20 specifically
      uri = URI.parse(ENV['DATABASE_URL'])
      db_real_path = CGI.unescape(uri.path)
      FileUtils.mkdir_p(File.dirname(db_real_path))
      
      db_config = {
        adapter: "sqlite3",
        database: db_real_path,
        pool: 20,
        timeout: 5000
      }
    else
      # Ensure the local directory exists
      FileUtils.mkdir_p("db")
      db_config = {
        adapter: "sqlite3",
        database: "db/development.sqlite3",
        pool: 20,
        timeout: 5000
      }
    end

    ActiveRecord::Base.establish_connection(db_config)

    # Optimization: Enable WAL mode for SQLite to handle concurrency
    ActiveRecord::Base.connection.execute("PRAGMA journal_mode=WAL;")
    ActiveRecord::Base.connection.execute("PRAGMA synchronous=NORMAL;")
    ActiveRecord::Base.connection.execute("PRAGMA busy_timeout=5000;")
    
    # Run migrations if pending
    # In AR 5.2 MigrationContext takes only the migrations path
    context = ActiveRecord::MigrationContext.new("db/migrate")
    if context.needs_migration?
      puts "Pending migrations detected. Automating migration..."
      context.migrate
      puts "Database schema is now up to date."
    end
  rescue => e
    puts "Database initialization warning: #{e.message}"
  end
end

# Initialize Client
$client = BrightspaceClient.new
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
end

# ==========================================
# Routes
# ==========================================

before do
  # Allow access to setup and public files without being "configured"
  return if ['/setup', '/favicon.ico', '/logo.png', '/auth/login'].include?(request.path_info) || request.path_info.start_with?('/public')
  
  if !configured?
    redirect '/setup'
  end

  @user_prefs = UserPreference.current
  
  # Auto-fetch name from Brightspace if we still have the default or empty
  if @user_prefs.display_name == "User" || @user_prefs.display_name.nil?
    user_data = $client.get_who_am_i
    if user_data && user_data['DisplayName']
      @user_prefs.update(display_name: user_data['DisplayName'])
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
    historic_units: params[:historic_units]
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
  cookies = BrightspaceAuthHelper.fetch_cookies(host)
  
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
  @course = Course.find_by(org_unit_id: @course_id)
  
  # If not in DB, try to fetch info from API to populate name
  if @course.nil? || @course.name.to_s.match?(/^\d+$/)
    course_info = $client.get_org_unit(@course_id)
    if course_info
      @course_name = course_info['Name']
      # Proactively create the course record so we have it next time
      @course = Course.create(
        org_unit_id: @course_id,
        name: @course_name,
        code: course_info['Code']
      ) rescue nil
    else
      @course_name = "Course #{@course_id}"
    end

    # If course exists but had a numeric name, update it
    if @course && @course_name != @course.name && !@course_name.match?(/^\d+$/)
      @course.update(name: @course_name)
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
          Course.find_by(org_unit_id: oid)&.update(name: info['Name']) if info && info['Name']
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
  @all_semesters = @courses.map { |c| c.respond_to?(:semester) ? c.semester : nil }.compact.uniq.sort
  @current_semester = @courses.map { |c| c.respond_to?(:semester) ? c.semester : nil }.compact.group_by(&:itself).values.max_by(&:size)&.first
  
  # Filter course list by semester if requested or persistent preference
  @semester_filter = params[:semester] || @user_prefs.default_semester || @current_semester
  
  # If the user explicitly sets a filter, persist it
  if params[:semester] && params[:semester] != @user_prefs.default_semester
    @user_prefs.update(default_semester: params[:semester])
  end

  if @semester_filter && @semester_filter != 'all'
    @display_courses = @courses.select { |c| (c.respond_to?(:semester) ? c.semester : nil) == @semester_filter }
  else
    @display_courses = @courses
  end
  
  if @current_semester
    @semester_courses = @courses.select { |c| (c.respond_to?(:semester) ? c.semester : nil) == @current_semester }
    @semester_grades = []
    
    # Weighting GPA by Course Units (Credits)
    total_weighted_points = 0.0
    semester_units = 0
    
    @semester_courses.each do |c|
      # Optimization: Perform calculation once
      stats = Grade.calculate_weighted_total(c.org_unit_id) rescue nil
      next unless stats

      # Treat 0 confidence courses as "not yet started" and exclude from dashboard analytics 
      # unless user specifically wants to see them
      if stats[:confidence] > 0 || stats[:all_possible_points] > 0
        sg_item = { 
          course: c,
          stats: stats
        }
        @semester_grades << sg_item

        # USM Calculation: GPA Points = (Scale Value * Units)
        course_gpa_value = Grade.to_gpa(stats[:score])
        course_units = c.units || 3
        
        total_weighted_points += (course_gpa_value * course_units)
        semester_units += course_units
      end
    end

    # --- HISTORIC CUMULATIVE GPA ---
    # Merge current semester with university reported history
    historic_gpa = @user_prefs.historic_gpa || 0.0
    historic_units = @user_prefs.historic_units || 0
    
    cumulative_points = (historic_gpa * historic_units) + total_weighted_points
    cumulative_units = historic_units + semester_units
    
    @overall_gpa = cumulative_units > 0 ? (cumulative_points / cumulative_units) : 0.0

    # --- MAX POTENTIAL SEMESTER GPA ---
    total_max_weighted_points = (historic_gpa * historic_units)
    @semester_grades.each do |sg|
        course_units = sg[:course].units || 3
        max_course_gpa = Grade.to_gpa(sg[:stats][:max_potential_score])
        total_max_weighted_points += (max_course_gpa * course_units)
    end
    @max_potential_gpa = cumulative_units > 0 ? (total_max_weighted_points / cumulative_units) : 0.0
  end
  # --------------------------

  @recent_notifications = Notification.where(is_read: false).order(date: :desc, id: :desc).limit(10)
  
  @sync_status = $client.sync_status
  
  erb :dashboard
end

# Sync Status Endpoint
get '/sync/status' do
  content_type :json
  $client.sync_status.to_json
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

# Assignments
get '/course/:id/assignments' do
  @active_tab = 'assignments'
  @breadcrumb_trail = [{ title: 'Assignments', url: "/course/#{@course_id}" }]
  @assignments = Assignment.where(course_id: @course_id).order(due_date: :asc)
  
  # Fallback if empty
  if @assignments.empty?
    @assignments_raw = $client.get_assignments(@course_id)
  end

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

# Enhanced Assignment Details
get '/course/:id/assignments/:assignment_id' do
  @active_tab = 'assignments'
  @assignment_id = params[:assignment_id]
  @assignment = $client.get_assignment(@course_id, @assignment_id)
  @feedback = $client.get_assignment_feedback(@course_id, @assignment_id)
  @rubrics = $client.get_assignment_rubrics(@course_id, @assignment_id)
  @submission_data = $client.get_assignment_submissions(@course_id, @assignment_id)
  
  sub_group = nil
  if @submission_data.is_a?(Array) && !@submission_data.empty?
      sub_group = @submission_data[0]
      @submissions = sub_group['Submissions']
      if (@feedback.nil? || (@feedback['Feedback']&.empty? rescue true))
          @feedback = sub_group['Feedback']
      end
  end

  # Check for rubrics in the submission data if not found in specific endpoint
  puts "DEBUG: Initial @rubrics: #{@rubrics.inspect}"
  if (@rubrics.nil? || @rubrics.empty?) && sub_group && sub_group['Rubrics']
    @rubrics = sub_group['Rubrics']
    puts "DEBUG: Fallback to sub_group: #{@rubrics.inspect}"
  end

  # Fallback to Rubric Definition if no assessment rubric is found
  if (@rubrics.nil? || @rubrics.empty?) && @assignment['Assessment'] && @assignment['Assessment']['Rubrics']
    @rubrics = @assignment['Assessment']['Rubrics']
    puts "DEBUG: Fallback to @assignment['Assessment']: #{@rubrics.inspect}"
  end

  if (@feedback.nil? || (@feedback['Feedback']&.empty? rescue true))
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

# Announcements
get '/course/:id/announcements' do
  @active_tab = 'announcements'
  @breadcrumb_trail = [{ title: 'Announcements', url: "/course/#{@course_id}/announcements" }]
  @news = $client.get_news(@course_id)
  erb :announcements
end

# Notifications
get '/notifications' do
  @active_tab = 'notifications'
  @courses = $client.get_enrollments
  @user = $client.get_who_am_i

  # Handle breadcrumb context
  if params[:from_course]
    @context_course = Course.find_by(org_unit_id: params[:from_course])
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
  @notifications = query.offset(offset).limit(@per_page)

  # Background check: If any semesters are missing, try to fill them
  Thread.new do
    ActiveRecord::Base.connection_pool.with_connection do
      Notification.where(semester: [nil, ""]).where.not(course_name: nil).find_each do |n|
        sem = $client.extract_semester_from_name(n.course_name)
        n.update_column(:semester, sem) if sem
      end
    end
  end

  erb :notifications
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

  redirect back
end

post '/notifications/:id/mark_unread' do
  notification = Notification.find(params[:id])
  notification.update(is_read: false)
  redirect back
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

# Grades
get '/course/:id/grades' do
  @active_tab = 'grades'
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
  
  path = "/#{path}" unless path.start_with?('/')
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
  files = collect_all_files(mod, safe_title)
  
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

# Start Server
Sinatra::Application.run! if __FILE__ == $0
