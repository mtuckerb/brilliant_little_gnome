require 'sinatra'
require 'sinatra/activerecord'
require 'active_support/time'
require 'time'
require 'zip'
require 'tempfile'
require 'icalendar'

require_relative 'lib/brightspace/client'
require_relative 'helpers/course_helpers'
require_relative 'models/notification'
require_relative 'models/api_cache'
require_relative 'models/download_job'
require_relative 'models/user_preference'

# Basic Configuration
set :port, 4567
set :views, File.join(File.dirname(__FILE__), 'views')

# Initialize Client
$client = BrightspaceClient.new

# Helpers
helpers CourseHelpers

helpers do
  def configured?
    # Check if we have at least a cookie or token
    $client.authenticated?
  end

  def format_date(date, format = "%b %d, %Y %I:%M %p")
    return "Recently" if date.nil?
    d = date.is_a?(String) ? Time.parse(date) : date
    
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
  return if ['/setup', '/favicon.ico'].include?(request.path_info) || request.path_info.start_with?('/public')
  
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
    time_zone: params[:time_zone]
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

before '/course/:id*' do
  redirect '/' unless $client.authenticated?
  
  @user = $client.get_who_am_i
  @course_id = params[:id]

  # Try to load course from normalized table
  @course = Course.find_by(org_unit_id: @course_id)
  @course_name = @course ? @course.name : "Course #{@course_id}"
  
  # Fetch TOC (can still use cache/API for complexity)
  @toc = $client.get_toc(@course_id)
  
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
  @courses = Course.all.unscope(:order).order(is_pinned: :desc, last_accessed_at: :desc)
  
  # Fallback if DB is empty
  if @courses.empty?
    @courses_raw = $client.get_enrollments
  end

  @recent_notifications = Notification.where(is_read: false).order(date: :desc, id: :desc).limit(10)
  
  erb :dashboard
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
  @breadcrumb_trail = [{ title: 'Assignments', url: "/course/#{@course_id}/assignments" }]
  @assignments = $client.get_assignments(@course_id)
  erb :assignments
end

# Export Assignments to ICS
get '/course/:id/assignments/export.ics' do
  assignments = $client.get_assignments(@course_id) || []
  
  cal = Icalendar::Calendar.new
  assignments.each do |a|
    next unless a['DueDate']
    
      event_start = Time.parse(a['DueDate'])
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
  @submission_data = $client.get_assignment_submissions(@course_id, @assignment_id)
  
  if @submission_data.is_a?(Array) && !@submission_data.empty?
      sub_group = @submission_data[0]
      @submissions = sub_group['Submissions']
      if (@feedback.nil? || (@feedback['Feedback']&.empty? rescue true))
          @feedback = sub_group['Feedback']
      end
  end

  if (@feedback.nil? || (@feedback['Feedback']&.empty? rescue true))
    grades = $client.get_grades(@course_id)
    @grade_entry = grades.find { |g| g['GradeObjectIdentifier'] == @assignment['GradeItemId'].to_s } if grades && @assignment['GradeItemId']
  end

  @breadcrumb_trail = [
    { title: 'Assignments', url: "/course/#{@course_id}/assignments" },
    { title: @assignment['Name'], url: "/course/#{@course_id}/assignments/#{@assignment_id}" }
  ]
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
  
  # Trigger a quick background sync for news/grades when viewing notifications
  Thread.new { $client.sync_notifications(@courses, @user) }

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
    Notification.where(semester: [nil, ""]).where.not(course_name: nil).find_each do |n|
      sem = $client.extract_semester_from_name(n.course_name)
      n.update_column(:semester, sem) if sem
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
  @grades = $client.get_grades(@course_id)
  erb :grades
end

# Discussions
get '/course/:id/discussions' do
  @active_tab = 'discussions'
  @topics = $client.get_all_topics(@course_id)
  @breadcrumb_trail = [{ title: 'Discussions', url: "/course/#{@course_id}/discussions" }]
  erb :discussions
end

# Discussion Topics
get '/course/:id/discussions/:forum_id/topics' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @forum = $client.get_discussion_forum(@course_id, @forum_id)
  @topics = $client.get_discussion_topics(@course_id, @forum_id)
  @breadcrumb_trail = [
    { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
    { title: @forum['Name'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics" }
  ]
  erb :discussion_topics
end

# Discussion Threads
get '/course/:id/discussions/:forum_id/topics/:topic_id' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @topic_id = params[:topic_id]
  
  @forum = $client.get_discussion_forum(@course_id, @forum_id)
  @topic = $client.get_discussion_topic(@course_id, @forum_id, @topic_id)
  @evaluation = $client.get_discussion_evaluation(@course_id, @forum_id, @topic_id)
  
  @breadcrumb_trail = [
    { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
    { title: @topic['Name'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}" }
  ]
  
  @threads_data = $client.get_discussion_threads(@course_id, @forum_id, @topic_id)
  @threads = @threads_data.is_a?(Hash) ? (@threads_data['Items'] || []) : (@threads_data || [])
  
  @threads_with_posts = @threads.sort_by { |t| t['IsPinned'] ? 0 : 1 }.map do |thread|
    posts_data = $client.get_thread_posts(@course_id, @forum_id, @topic_id, thread['ThreadId'])
    posts = posts_data.is_a?(Hash) ? (posts_data['Items'] || []) : (posts_data || [])
    {
      thread: thread,
      post_tree: build_post_tree(posts)
    }
  end

  erb :discussion_threads
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
    { status: job.status, progress: job.progress }.to_json
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
