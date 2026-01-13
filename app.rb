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

helpers do
  def truncate_text(text, max_length = 20)
    return text if text.nil? || text.length <= max_length
    text[0...max_length-1] + "…"
  end
end

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
  @courses = $client.get_enrollments
  @all_news = $client.get_all_news(@courses)
  
  erb :dashboard
end

# Course Overview / Syllabus
get '/course/:id' do
  @active_tab = 'overview'
  @breadcrumb_trail = [{ title: 'Overview', url: "/course/#{@course_id}" }]
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

# Discussion Threads (The "Everything" Topic View)
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
  
  @threads_data = $client.get_discussion_threads(@course_id, @forum_id, @topic_id, force_refresh: params[:force_refresh] == 'true')

  # DEBUG: Log the raw response to help troubleshoot
  File.write("debugging/threads_#{Time.now.to_i}.json", @threads_data.to_json) if @threads_data
  
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
  @breadcrumb_trail = [
    { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
    { title: @topic['Name'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}" },
    { title: @thread['Title'], url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}/threads/#{@thread_id}" }
  ]
  @posts_data = $client.get_thread_posts(@course_id, @forum_id, @topic_id, @thread_id)
  @posts = @posts_data.is_a?(Hash) ? (@posts_data['Items'] || []) : (@posts_data || [])

  erb :discussion_posts
end


# Generic Download Route for explicit paths
get '/course/:id/download' do
  path = params[:path]
  name = params[:name] || "download"
  
  puts "[Britespace Download] Requested path: #{path}"
  
  # Ensure path starts with /
  path = "/#{path}" unless path.start_with?('/')
  
  http_resp = $client.download_file(path)
  
  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type'] || 'application/octet-stream'
    # Use URI encoding for the filename in the header to handle spaces and special chars safely
    safe_name = name.gsub(/[^0-9A-Za-z.\- ]/, '_')
    headers["Content-Disposition"] = "attachment; filename=\"#{safe_name}\""
    http_resp.body
  else
    status_code = http_resp ? http_resp.code : 'No Response'
    puts "[Britespace Download] FAILED with status #{status_code} for path #{path}"
    "Download failed: Status #{status_code}. <br>Detailed Path: #{path}"
  end
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
  files = collect_everything(@course_id, $client, @toc)
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
  
  # For a single module, we'll just use its name as the folder
  folder_name = mod_obj ? mod_obj['Title'].gsub(/[^0-9a-z]/i, '_') : "Module"
  files = collect_all_files(mod_obj, folder_name)
  
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
