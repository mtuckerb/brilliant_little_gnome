require 'sinatra'
require 'uri'
require 'net/http'
require 'json'
require 'base64'

# Basic Configuration
set :port, 4567

# ==========================================
# Logic from brightspace_poc.rb (Refactored)
# ==========================================

class BrightspaceClient
  attr_accessor :token, :cookie_string

  def initialize
    @host = ENV['BS_HOST']
    @client_id = ENV['BS_CLIENT_ID']
    @client_secret = ENV['BS_CLIENT_SECRET']
    @redirect_uri = ENV['BS_REDIRECT_URI'] || "http://localhost:4567/callback"
    @api_version = "1.40"
    
    # Try loading from cookies.txt (e.g. for pre-seeded dev)
    load_cookies_from_file if File.exist?('cookies.txt')
  end

  def load_cookies_from_file
    content = File.read('cookies.txt').strip
    return if content.empty?

    if content.start_with?('ey')
      @token = content
    else
      @cookie_string = content
    end
  end

  def auth_url
    "https://#{@host}/oauth2/auth?" + URI.encode_www_form({
      response_type: 'code',
      client_id: @client_id,
      redirect_uri: @redirect_uri,
      scope: 'core:*:* enrollments:*:* content:*:* grades:*:*'
    })
  end

  def exchange_code(code)
    uri = URI("https://#{@host}/core/connect/token")
    request = Net::HTTP::Post.new(uri)
    request.content_type = 'application/x-www-form-urlencoded'
    
    auth_str = Base64.strict_encode64("#{@client_id}:#{@client_secret}")
    request['Authorization'] = "Basic #{auth_str}"
    
    request.set_form_data({
      'grant_type' => 'authorization_code',
      'redirect_uri' => @redirect_uri,
      'code' => code
    })

    response = Net::HTTP.start(uri.hostname, uri.port, use_ssl: true) do |http|
      http.request(request)
    end

    if response.code == '200'
      data = JSON.parse(response.body)
      @token = data['access_token']
      return true
    end
    false
  end

  def authenticated?
    !@token.nil? || !@cookie_string.nil?
  end

  def get_who_am_i
    do_get("/d2l/api/lp/#{@api_version}/users/whoami")
  end

  def get_enrollments
    response = do_get("/d2l/api/lp/#{@api_version}/enrollments/myenrollments/")
    return [] unless response
    
    items = response['Items'] || response
    # Filter for Course Offerings and active courses
    items.select do |i| 
      (i.dig('OrgUnit', 'Type', 'Code') == 'Course Offering' || 
       i.dig('OrgUnit', 'Type', 'Name') == 'Course Offering')
    end
  end

  # NEW: Fetch Table of Contents (Modules and Topics)
  def get_toc(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/content/toc")
  end

  # NEW: Fetch Assignments
  def get_assignments(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/assignments/")
  end

  # NEW: Fetch Grades for the current user
  def get_grades(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/grades/values/myGradeValues/")
  end

  # NEW: Fetch Discussion Forums
  def get_discussions(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/discussions/forums/")
  end

  # NEW: Fetch Overview (often serves as Syllabus)
  def get_overview(org_unit_id)
    do_get("/d2l/api/le/#{@api_version}/#{org_unit_id}/overview")
  end

  # NEW: Download Helper (returns headers and body stream ideally, here returns full body for simplicity)
  def download_file(path)
    return nil unless authenticated?

    uri = URI("https://#{@host}#{path}")
    request = Net::HTTP::Get.new(uri)
    
    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string
    end

    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true
    
    http.request(request)
  end

  private

  def do_get(path)
    return nil unless authenticated?

    uri = URI("https://#{@host}#{path}")
    request = Net::HTTP::Get.new(uri)
    
    if @token
      request['Authorization'] = "Bearer #{@token}"
    elsif @cookie_string
      request['Cookie'] = @cookie_string
    end
    
    request['Accept'] = 'application/json'

    http = Net::HTTP.new(uri.host, uri.port)
    http.use_ssl = true
    
    response = http.request(request)
    
    if response.code == '200'
      JSON.parse(response.body)
    else
      puts "Error parsing #{path}: #{response.code} #{response.message}"
      nil
    end
  end
end

# Initialize Client
$client = BrightspaceClient.new

# ==========================================
# Routes
# ==========================================

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

# NEW: Course Details Route (Main/Syllabus)
get '/course/:id' do
  redirect '/' unless $client.authenticated?
  
  @course_id = params[:id]
  @active_tab = 'overview'
  @toc = $client.get_toc(@course_id)
  @overview = $client.get_overview(@course_id)
  
  if @toc && @toc['Modules']
    @syllabus_module = @toc['Modules'].find { |m| m['Title'].downcase.include?('syllabus') } || 
                       @toc['Modules'].find { |m| m['Title'].downcase.include?('overview') }
  end

  # Get course name
  enrollments = $client.get_enrollments
  course_obj = enrollments.find { |e| e['OrgUnit']['Id'].to_s == @course_id }
  @course_name = course_obj ? course_obj['OrgUnit']['Name'] : "Course #{@course_id}"

  erb :course_detail
end

# NEW: Specific Module Route
get '/course/:id/module/:module_id' do
  redirect '/' unless $client.authenticated?
  
  @course_id = params[:id]
  @module_id = params[:module_id]
  @active_tab = "module_#{@module_id}"
  @toc = $client.get_toc(@course_id)

  # Find the specific module in the TOC tree
  def find_module(modules, id)
    return nil unless modules
    modules.each do |m|
      return m if m['ModuleId'].to_s == id.to_s
      found = find_module(m['Modules'], id)
      return found if found
    end
    nil
  end
  
  @module = find_module(@toc['Modules'], @module_id) if @toc

  enrollments = $client.get_enrollments
  course_obj = enrollments.find { |e| e['OrgUnit']['Id'].to_s == @course_id }
  @course_name = course_obj ? course_obj['OrgUnit']['Name'] : "Course #{@course_id}"
  
  erb :module_detail
end

# NEW: Course Assignments
get '/course/:id/assignments' do
  redirect '/' unless $client.authenticated?
  
  @course_id = params[:id]
  @active_tab = 'assignments'
  @toc = $client.get_toc(@course_id)
  @assignments = $client.get_assignments(@course_id)
  
  enrollments = $client.get_enrollments
  course_obj = enrollments.find { |e| e['OrgUnit']['Id'].to_s == @course_id }
  @course_name = course_obj ? course_obj['OrgUnit']['Name'] : "Course #{@course_id}"
  
  erb :assignments
end

# NEW: Course Grades
get '/course/:id/grades' do
  redirect '/' unless $client.authenticated?
  
  @course_id = params[:id]
  @active_tab = 'grades'
  @toc = $client.get_toc(@course_id)
  @grades = $client.get_grades(@course_id)
  
  enrollments = $client.get_enrollments
  course_obj = enrollments.find { |e| e['OrgUnit']['Id'].to_s == @course_id }
  @course_name = course_obj ? course_obj['OrgUnit']['Name'] : "Course #{@course_id}"
  
  erb :grades
end

# NEW: Course Discussions
get '/course/:id/discussions' do
  redirect '/' unless $client.authenticated?
  
  @course_id = params[:id]
  @active_tab = 'discussions'
  @toc = $client.get_toc(@course_id)
  @forums = $client.get_discussions(@course_id)
  
  enrollments = $client.get_enrollments
  course_obj = enrollments.find { |e| e['OrgUnit']['Id'].to_s == @course_id }
  @course_name = course_obj ? course_obj['OrgUnit']['Name'] : "Course #{@course_id}"
  
  erb :discussions
end

# NEW: Download Route (Overview Attachment)
get '/course/:id/overview/download' do
  redirect '/' unless $client.authenticated?
  
  course_id = params[:id]
  http_resp = $client.download_file("/d2l/api/le/1.40/#{course_id}/overview/attachment")
  
  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type']
    headers["Content-Disposition"] = http_resp['Content-Disposition'] || "attachment; filename=\"syllabus.pdf\""
    http_resp.body
  else
    "Download failed: #{http_resp ? http_resp.code : 'Unknown error'}"
  end
end

# NEW: Download Route (Topic/File)
get '/course/:course_id/topic/:topic_id/download' do
  redirect '/' unless $client.authenticated?
  
  course_id = params[:course_id]
  topic_id = params[:topic_id]
  
  # The generic file download endpoint for a topic
  http_resp = $client.download_file("/d2l/api/le/1.40/#{course_id}/content/topics/#{topic_id}/file")

  if http_resp && http_resp.code == '200'
    content_type http_resp['Content-Type']
    headers["Content-Disposition"] = http_resp['Content-Disposition'] || "attachment; filename=\"file_#{topic_id}.pdf\""
    http_resp.body
  else
    "Download failed: #{http_resp ? http_resp.code : 'Unknown error'}"
  end
end

# ==========================================
# Views (Inline)
# ==========================================

__END__

@@ layout
<!DOCTYPE html>
<html>
<head>
  <title>Brightspace Student Dashboard</title>
  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/bulma/0.9.4/css/bulma.min.css">
  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.0.0/css/all.min.css">
  <style>
    body { background: #fdfdfd; min-height: 100vh; }
    .main-content { padding: 40px; }
    .course-row { transition: background-color 0.2s; cursor: pointer; }
    .course-row:hover { background-color: #f5f5f5; }
    .navbar { box-shadow: 0 1px 3px rgba(0,0,0,0.1); }
    .module-list li { padding: 8px 12px; border-left: 3px solid transparent; cursor: pointer; }
    .module-list li:hover { background: #f5f5f5; border-left-color: #00d1b2; }
    .module-list li.active { background: #eefdfd; border-left-color: #00d1b2; font-weight: bold; }
    .menu-label { margin-top: 1.5em; }
  </style>
</head>
<body>
  <nav class="navbar is-white">
    <div class="navbar-brand">
       <a class="navbar-item is-size-4 has-text-weight-bold has-text-primary" href="/dashboard">
         <i class="fas fa-graduation-cap mr-2"></i> Britespace
       </a>
    </div>
    <div class="navbar-end">
       <div class="navbar-item">
          <% if @user %>
             <span class="tag is-light is-medium">
               <i class="fas fa-user mr-2"></i> <%= "#{@user['FirstName']} #{@user['LastName']}" %>
             </span>
          <% end %>
       </div>
    </div>
  </nav>

  <div class="main-content container">
    <%= yield %>
  </div>
</body>
</html>

@@ login
<section class="hero is-primary is-fullheight-with-navbar">
  <div class="hero-body">
    <div class="container has-text-centered">
      <h1 class="title is-1">
        Student Dashboard
      </h1>
      <h2 class="subtitle is-3">
        Your simplified learning experience.
      </h2>
      <a href="/login" class="button is-white is-large is-rounded">
        <strong><i class="fas fa-sign-in-alt mr-2"></i> Login with Brightspace</strong>
      </a>
      <p class="mt-4 is-size-7">Or ensure 'cookies.txt' is configured locally.</p>
    </div>
  </div>
</section>

@@ dashboard
<div class="level mb-6">
  <div class="level-left">
    <h1 class="title"><i class="fas fa-book-open mr-2"></i> My Course List</h1>
  </div>
</div>

<% if @courses.empty? %>
  <div class="notification is-warning">
    No active courses found.
  </div>
<% else %>

  <div class="box p-0">
    <table class="table is-fullwidth is-hoverable">
      <thead>
        <tr>
          <th>Course Name</th>
          <th>Code</th>
          <th>ID</th>
          <th class="has-text-right">Action</th>
        </tr>
      </thead>
      <tbody>
        <% @courses.each do |c| %>
          <tr class="course-row" onclick="window.location='/course/<%= c['OrgUnit']['Id'] %>'">
            <td class="has-text-weight-medium is-size-5"><%= c['OrgUnit']['Name'] %></td>
            <td class="has-text-grey"><%= c['OrgUnit']['Code'] %></td>
            <td class="is-family-code has-text-grey-light"><%= c['OrgUnit']['Id'] %></td>
            <td class="has-text-right">
              <a href="/course/<%= c['OrgUnit']['Id'] %>" class="button is-small is-primary is-light">
                View Content <i class="fas fa-arrow-right ml-2"></i>
              </a>
            </td>
          </tr>
        <% end %>
      </tbody>
    </table>
  </div>
  
<% end %>

@@ course_detail
<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/dashboard">Courses</a></li>
    <li class="is-active"><a href="#" aria-current="page"><%= @course_name %></a></li>
  </ul>
</nav>

<h1 class="title mb-4"><%= @course_name %></h1>

<div class="columns">
  <!-- Left Sidebar: Menu -->
  <div class="column is-3">
    <aside class="menu">
      <p class="menu-label">General</p>
      <ul class="menu-list">
        <li><a href="/course/<%= @course_id %>" class="<%= 'is-active' if @active_tab == 'overview' %>">
          <span class="icon is-small"><i class="fas fa-info-circle"></i></span> Syllabus / Overview</a>
        </li>
        <li><a href="/course/<%= @course_id %>/assignments" class="<%= 'is-active' if @active_tab == 'assignments' %>">
          <span class="icon is-small"><i class="fas fa-tasks"></i></span> Assignments</a>
        </li>
        <li><a href="/course/<%= @course_id %>/grades" class="<%= 'is-active' if @active_tab == 'grades' %>">
          <span class="icon is-small"><i class="fas fa-poll"></i></span> Grades</a>
        </li>
        <li><a href="/course/<%= @course_id %>/discussions" class="<%= 'is-active' if @active_tab == 'discussions' %>">
          <span class="icon is-small"><i class="fas fa-comments"></i></span> Discussions</a>
        </li>
      </ul>
      
      <p class="menu-label">Table of Contents</p>
      <ul class="menu-list module-list">
        <% if @toc && @toc['Modules'] %>
          <% @toc['Modules'].each do |mod| %>
            <li>
              <a><%= mod['Title'] %></a>
            </li>
          <% end %>
        <% else %>
          <li><span class="has-text-grey-light">No modules found.</span></li>
        <% end %>
      </ul>
    </aside>
  </div>
  
  <!-- Right Content Area: Details -->
  <div class="column is-9">
    <div class="box">
      <!-- Overview Section -->
      <% if @overview %>
         <div class="level">
             <div class="level-left">
                <h4 class="title is-4">Syllabus / Overview</h4>
             </div>
             
             <% if @overview['Attachment'] %>
             <div class="level-right">
                <a href="/course/<%= @course_id %>/overview/download" class="button is-link is-small" target="_blank">
                   <span class="icon is-small mr-1"><i class="fas fa-download"></i></span>
                   Download <%= @overview['Attachment']['FileName'] %> 
                </a>
             </div>
             <% end %>
         </div>
         
         <div class="content">
            <% 
               desc = @overview['Description']
               desc_text = desc.is_a?(Hash) ? (desc['Html'] || desc['Text'] || "") : desc.to_s
            %>
            
            <% unless desc_text.strip.empty? %>
              <%= desc_text %> 
            <% else %>
              <p class="has-text-grey-light">No description provided and no "Overview" attachment.</p>
              
              <% if @syllabus_module %>
                <hr />
                <h5 class="title is-5 is-flex is-align-items-center">
                  <span class="icon has-text-primary mr-2"><i class="fas fa-folder-open"></i></span> 
                  Found "Syllabus" Module
                </h5>
                <p class="mb-4">This course uses a module for its syllabus. Here are the files:</p>
                
                <% if @syllabus_module['Topics'] && !@syllabus_module['Topics'].empty? %>
                   <div class="list is-hoverable">
                     <% @syllabus_module['Topics'].each do |topic| %>
                       <% next unless topic['Type'] == 1 %>
                       <div class="list-item">
                          <div class="is-flex is-justify-content-space-between is-align-items-center">
                             <div>
                               <span class="icon is-small has-text-info mr-2"><i class="fas fa-file-pdf"></i></span>
                               <span class="has-text-weight-medium"><%= topic['Title'] %></span>
                             </div>
                             <a href="/course/<%= @course_id %>/topic/<%= topic['Id'] %>/download" class="button is-small is-link is-outlined" target="_blank">
                               Download
                             </a>
                          </div>
                       </div>
                     <% end %>
                   </div>
                <% else %>
                   <div class="notification is-light">No files found inside the Syllabus module.</div>
                <% end %>
              <% end %>
            <% end %>
         </div>
      <% else %>
         <h4 class="title is-4">Select a module...</h4>
         <p class="has-text-grey">Click on a module on the left to view its topics.</p>
      <% end %>
    </div>
  </div>
</div>

@@ assignments
<%= erb :course_detail_header %>
<div class="columns">
  <div class="column is-3">
    <%= erb :sidebar %>
  </div>
  <div class="column is-9">
    <div class="box">
      <h4 class="title is-4"><i class="fas fa-tasks mr-2"></i> Assignments</h4>
      <% if @assignments && !@assignments.empty? %>
        <table class="table is-fullwidth is-hoverable">
          <thead>
            <tr>
              <th>Name</th>
              <th>Due Date</th>
              <th class="has-text-right">Action</th>
            </tr>
          </thead>
          <tbody>
            <% @assignments.each do |a| %>
              <tr>
                <td><strong><%= a['Name'] %></strong></td>
                <td><%= a['DueDate'] ? Time.parse(a['DueDate']).strftime("%B %d, %Y %I:%M %p") : "No due date" %></td>
                <td class="has-text-right">
                  <a href="#" class="button is-small is-primary is-light">View</a>
                </td>
              </tr>
            <% end %>
          </tbody>
        </table>
      <% else %>
        <p class="has-text-grey">No assignments found for this course.</p>
      <% end %>
    </div>
  </div>
</div>

@@ grades
<%= erb :course_detail_header %>
<div class="columns">
  <div class="column is-3">
    <%= erb :sidebar %>
  </div>
  <div class="column is-9">
    <div class="box">
      <h4 class="title is-4"><i class="fas fa-poll mr-2"></i> Grades</h4>
      <% if @grades && !@grades.empty? %>
        <table class="table is-fullwidth">
          <thead>
            <tr>
              <th>Item</th>
              <th>Points</th>
              <th>Grade</th>
            </tr>
          </thead>
          <tbody>
            <% @grades.each do |g| %>
              <tr>
                <td><%= g['GradeObjectName'] %></td>
                <td><%= g['DisplayedGrade'] %></td>
                <td>
                   <span class="tag is-info"><%= g['PointsNumerator'] %> / <%= g['PointsDenominator'] %></span>
                </td>
              </tr>
            <% end %>
          </tbody>
        </table>
      <% else %>
        <p class="has-text-grey">No grade values found yet.</p>
      <% end %>
    </div>
  </div>
</div>

@@ discussions
<%= erb :course_detail_header %>
<div class="columns">
  <div class="column is-3">
    <%= erb :sidebar %>
  </div>
  <div class="column is-9">
    <div class="box">
      <h4 class="title is-4"><i class="fas fa-comments mr-2"></i> Discussion Forums</h4>
      <% if @forums && !@forums.empty? %>
        <% @forums.each do |forum| %>
          <div class="content mb-6">
            <h5 class="title is-5"><%= forum['Name'] %></h5>
            <p><%= forum['Description']['Text'] if forum['Description'] %></p>
            <hr />
          </div>
        <% end %>
      <% else %>
        <p class="has-text-grey">No discussion forums found.</p>
      <% end %>
    </div>
  </div>
</div>

@@ course_detail_header
<nav class="breadcrumb" aria-label="breadcrumbs">
  <ul>
    <li><a href="/dashboard">Courses</a></li>
    <li class="is-active"><a href="#" aria-current="page"><%= @course_name %></a></li>
  </ul>
</nav>
<h1 class="title mb-4"><%= @course_name %></h1>

@@ sidebar
<aside class="menu">
  <p class="menu-label">General</p>
  <ul class="menu-list">
    <li><a href="/course/<%= @course_id %>" class="<%= 'is-active' if @active_tab == 'overview' %>">
      <span class="icon is-small"><i class="fas fa-info-circle"></i></span> Syllabus / Overview</a>
    </li>
    <li><a href="/course/<%= @course_id %>/assignments" class="<%= 'is-active' if @active_tab == 'assignments' %>">
      <span class="icon is-small"><i class="fas fa-tasks"></i></span> Assignments</a>
    </li>
    <li><a href="/course/<%= @course_id %>/grades" class="<%= 'is-active' if @active_tab == 'grades' %>">
      <span class="icon is-small"><i class="fas fa-poll"></i></span> Grades</a>
    </li>
    <li><a href="/course/<%= @course_id %>/discussions" class="<%= 'is-active' if @active_tab == 'discussions' %>">
      <span class="icon is-small"><i class="fas fa-comments"></i></span> Discussions</a>
    </li>
  </ul>
</aside>
