require 'sinatra'
require 'time'
require 'zip'
require 'tempfile'
require 'icalendar'
require_relative 'lib/brightspace/client'
require_relative 'helpers/course_helpers'
require_relative 'models/download_job'

# Basic Configuration
set :port, 4567
set :views, File.join(File.dirname(__FILE__), 'views')

# Initialize Client
$client = BrightspaceClient.new

# Helpers
helpers CourseHelpers

# ==========================================
# Routes
# ==========================================

before '/course/:id*' do
  redirect '/' unless $client.authenticated?
  
  @course_id = params[:id]
  @toc = $client.get_toc(@course_id)
  
  # Fetch course name from enrollments (caching this would be better)
  enrollments = $client.get_enrollments
  course_obj = enrollments.find { |e| e['OrgUnit']['Id'].to_s == @course_id }
  @course_name = course_obj ? course_obj['OrgUnit']['Name'] : "Course #{@course_id}"
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
  @courses = $client.get_enrollments
  @all_news = $client.get_all_news(@courses)
  
  erb :dashboard
end

# Course Overview / Syllabus
get '/course/:id' do
  @active_tab = 'overview'
  @overview = $client.get_overview(@course_id)
  @syllabus_module = find_syllabus_module(@toc['Modules']) if @toc
  
  erb :course_detail
end

# Specific Module Details
get '/course/:id/module/:module_id' do
  @module_id = params[:module_id]
  @active_tab = "module_#{@module_id}"
  @module = find_module(@toc['Modules'], @module_id)
  @breadcrumbs = build_breadcrumbs(@toc['Modules'], @module_id) if @toc
  
  erb :module_detail
end

# Assignments
get '/course/:id/assignments' do
  @active_tab = 'assignments'
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
      e.description = "Assignment Due. Check Britespace for instructions."
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
  erb :assignment_detail
end

# Announcements
get '/course/:id/announcements' do
  @active_tab = 'announcements'
  @news = $client.get_news(@course_id)
  erb :announcements
end

# Grades
get '/course/:id/grades' do
  @active_tab = 'grades'
  @grades = $client.get_grades(@course_id)
  erb :grades
end

# Discussions
get '/course/:id/discussions' do
  @active_tab = 'discussions'
  @topics = $client.get_all_topics(@course_id)
  erb :discussions
end

# Discussion Topics
get '/course/:id/discussions/:forum_id/topics' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @forum = $client.get_discussion_forum(@course_id, @forum_id)
  @topics = $client.get_discussion_topics(@course_id, @forum_id)
  erb :discussion_topics
end

# Discussion Threads (The "Everything" Topic View)
get '/course/:id/discussions/:forum_id/topics/:topic_id' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @topic_id = params[:topic_id]
  
  @forum = $client.get_discussion_forum(@course_id, @forum_id)
  @topic = $client.get_discussion_topic(@course_id, @forum_id, @topic_id)
  @evaluation = $client.get_discussion_evaluation(@course_id, @forum_id, @topic_id)
  
  @threads_data = $client.get_discussion_threads(@course_id, @forum_id, @topic_id, force_refresh: true)

  # DEBUG: Log the raw response to help troubleshoot
  File.write("debug_threads_#{Time.now.to_i}.json", @threads_data.to_json) if @threads_data
  
  @threads = []
  if @threads_data.is_a?(Hash)
    @threads = @threads_data['Items'] || []
  elsif @threads_data.is_a?(Array)
    @threads = @threads_data
  end
  
  # Fetch posts for every thread to show the "Thread Tree" immediately
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

# Discussion Posts (Tree view of posts in a thread)
get '/course/:id/discussions/:forum_id/topics/:topic_id/threads/:thread_id' do
  @active_tab = 'discussions'
  @forum_id = params[:forum_id]
  @topic_id = params[:topic_id]
  @thread_id = params[:thread_id]
  
  @forum = $client.get_discussion_forum(@course_id, @forum_id)
  @topic = $client.get_discussion_topic(@course_id, @forum_id, @topic_id)
  @thread = $client.get_discussion_thread(@course_id, @forum_id, @topic_id, @thread_id)
  @posts_data = $client.get_thread_posts(@course_id, @forum_id, @topic_id, @thread_id)
  @posts = @posts_data.is_a?(Hash) ? (@posts_data['Items'] || []) : (@posts_data || [])
  
  erb :discussion_posts
end

# NEW: Search
get '/course/:id/search' do
  @query = params[:q]
  @active_tab = 'search'
  @results = search_toc(@toc['Modules'], @query) if @toc && @query
  erb :search
end

# Download Route (Overview Attachment)
get '/course/:id/overview/download' do
  http_resp = $client.download_file("/d2l/api/le/1.40/#{@course_id}/overview/attachment")
  
  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type']
    headers["Content-Disposition"] = http_resp['Content-Disposition'] || "attachment; filename=\"syllabus.pdf\""
    http_resp.body
  else
    "Download failed: #{http_resp ? http_resp.code : 'Unknown error'}"
  end
end

# Download Route (Topic/File)
get '/course/:id/topic/:topic_id/download' do
  topic_id = params[:topic_id]
  http_resp = $client.download_file("/d2l/api/le/1.40/#{@course_id}/content/topics/#{topic_id}/file")

  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type']
    headers["Content-Disposition"] = http_resp['Content-Disposition'] || "attachment; filename=\"file_#{topic_id}.pdf\""
    http_resp.body
  else
    "Download failed: #{http_resp ? http_resp.code : 'Unknown error'}"
  end
end

# ASYNC: Download All Files in a Course
get '/course/:id/download_all' do
  files = collect_course_files(@toc)
  if files.empty?
    return "No downloadable files found in this course."
  end

  job = DownloadJob.create(@course_id, files, $client)
  redirect "/job/#{job.id}"
end

# ASYNC: Download All Files in a Module
get '/course/:id/module/:module_id/download_all' do
  module_id = params[:module_id]
  mod_obj = find_module(@toc['Modules'], module_id)
  files = collect_all_files(mod_obj)
  
  if files.empty?
    return "No downloadable files found in this module."
  end

  job = DownloadJob.create(module_id, files, $client)
  redirect "/job/#{job.id}"
end

# Job Status View
get '/job/:id' do
  @job = DownloadJob.find(params[:id])
  unless @job
    return "Job not found."
  end
  erb :job_status
end

# Job JSON API (for polling)
get '/job/:id/status' do
  job = DownloadJob.find(params[:id])
  if job
    content_type :json
    { 
      status: job.status, 
      progress: job.progress, 
      total: job.total_files, 
      completed: job.completed_files,
      error: job.error 
    }.to_json
  else
    status 404
    { error: "Job not found" }.to_json
  end
end

# Download Finished ZIP
get '/job/:id/download' do
  job = DownloadJob.find(params[:id])
  if job && job.status == :completed && File.exist?(job.zip_path)
    send_file job.zip_path, :type => 'application/zip', :disposition => 'attachment', :filename => File.basename(job.zip_path)
  else
    "File not ready or job failed."
  end
end
