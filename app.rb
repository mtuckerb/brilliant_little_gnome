require 'sinatra'
require 'time'
require 'zip'
require 'tempfile'
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
  
  @user = $client.get_who_am_i
  @courses = $client.get_enrollments
  
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

# Grades
get '/course/:id/grades' do
  @active_tab = 'grades'
  @grades = $client.get_grades(@course_id)
  erb :grades
end

# Discussions
get '/course/:id/discussions' do
  @active_tab = 'discussions'
  @forums = $client.get_discussions(@course_id)
  erb :discussions
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
