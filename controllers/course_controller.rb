require_relative 'base_controller'

class CourseController < BaseController
  before '/course/:id*' do
    redirect '/' unless configured?
    @course_id = params[:id].to_s
    @course = Course.find_by(org_unit_id: @course_id)
    @course_name = @course&.name || "Course #{@course_id}"
    
    # TOC for Sidebar
    @toc = build_toc_tree(@course_id)
    if @toc['Modules'].empty?
      begin
        # Fetch live TOC data
        @toc = $client.get_toc(@course_id)
        # If we got data back, trigger a background sync to persist it
        if @toc['Modules'] && !@toc['Modules'].empty?
          Thread.new { ActiveRecord::Base.connection_pool.with_connection { $client.sync_course_content(@course_id, @toc) } }
        end
      rescue => e
        puts "[CourseController] Failed to fetch live TOC: #{e.message}"
      end
    end
  end

  get '/course/:id' do
    @active_tab = 'overview'
    @breadcrumb_trail = [{ title: 'Overview', url: "/course/#{@course_id}" }]
    # Shell rendering - data is loaded via JS in course_detail.erb
    erb :course_detail
  end

  get '/course/:id/assignments' do
    @active_tab = 'assignments'
    @breadcrumb_trail = [{ title: 'Assignments', url: "/course/#{@course_id}/assignments" }]
    @show_completed = params[:show_completed] == 'true'
    
    @assignments = Assignment.where(course_id: @course_id)
    @assignments = @assignments.where(completed: false) unless @show_completed
    @assignments = @assignments.order(due_date: :asc)
    
    erb :assignments
  end

  get '/course/:id/grades' do
    @active_tab = 'grades'
    @breadcrumb_trail = [{ title: 'Grades', url: "/course/#{@course_id}/grades" }]
    @grades = Grade.where(course_id: @course_id).order(Arel.sql("due_date ASC NULLS LAST"), name: :asc)
    @grade_stats = calculate_grade_stats(@course_id)
    erb :grades
  end

  get '/course/:id/discussions' do
    @active_tab = 'discussions'
    @forums = DiscussionForum.where(course_id: @course_id).order(name: :asc)
    @topics = DiscussionTopic.where(course_id: @course_id).order(sort_order: :asc)
    @breadcrumb_trail = [{ title: 'Discussions', url: "/course/#{@course_id}/discussions" }]
    erb :discussions
  end

  get '/course/:id/notifications' do
    @active_tab = 'notifications'
    @courses = $client.get_enrollments
    @user = $client.get_who_am_i
    @context_course = @course
    @breadcrumb_trail = [{ title: 'Notifications', url: "/course/#{@course_id}/notifications" }]
    
    # Seed the filter for the view
    params[:course_id] = @course_id
    
    erb :notifications
  end

  get '/course/:id/announcements' do
    @active_tab = 'announcements'
    @breadcrumb_trail = [{ title: 'Announcements', url: "/course/#{@course_id}/announcements" }]
    @announcements = Notification.where(course_id: @course_id, notification_type: 'News').order(date: :desc)
    
    # If empty, sync news
    if @announcements.empty?
      Thread.new { ActiveRecord::Base.connection_pool.with_connection { $client.sync_notifications($client.get_enrollments, $client.get_who_am_i) } }
    end
    
    erb :announcements
  end

  get '/course/:id/module/:module_id' do
    @module_id = params[:module_id]
    @active_tab = "module_#{@module_id}"
    @lineage = find_lineage(@toc['Modules'], @module_id) || []
    
    # Try to find module name for breadcrumbs
    module_obj = find_module(@toc['Modules'], @module_id)
    @module_title = module_obj ? module_obj['Title'] : "Module"
    
    @breadcrumb_trail = [
      { title: 'Content', url: "/course/#{@course_id}" },
      { title: @module_title, url: "/course/#{@course_id}/module/#{@module_id}" }
    ]
    
    erb :module_detail
  end

  get '/course/:id/search' do
    @query = params[:q]
    @active_tab = 'search'
    
    @toc = build_toc_tree(@course_id)
    @toc = $client.get_toc(@course_id) if @toc['Modules'].empty?
    
    @results = search_toc(@toc['Modules'], @query) if @toc && @query
    erb :search
  end

  # --- Course Actions ---
  post '/course/:id/update_units' do
    @course.update(units: params[:units]) if @course
    redirect back
  end

  post '/course/:id/update_color' do
    @course.update(custom_color: params[:custom_color]) if @course
    redirect back
  end

  post '/course/:id/update_target_grade' do
    @course.update(target_grade: params[:target_grade]) if @course
    redirect back
  end

  post '/course/:id/update_end_of_week' do
    @course.update(end_of_week_day: params[:end_of_week_day].to_i) if @course
    redirect back
  end

  post '/course/:id/grades/:grade_id/toggle_extra_credit' do
    grade = Grade.find_by(id: params[:grade_id], course_id: params[:id])
    grade.update(is_extra_credit: !grade.is_extra_credit) if grade
    
    if request.xhr?
      content_type :json
      { status: 'ok', is_extra_credit: grade.is_extra_credit }.to_json
    else
      redirect back
    end
  end

  post '/course/:id/assignments/bulk_optional' do
    ids = params[:ids]
    optional_value = params[:optional] == 'true'
    
    if ids && ids.is_a?(Array)
      Assignment.where(id: ids, course_id: params[:id]).update_all(optional: optional_value)
      if request.xhr?
        content_type :json
        { status: 'ok', updated: ids.size }.to_json
      else
        flash[:success] = "Updated #{ids.size} assignments"
        redirect back
      end
    else
      status 400
      { status: 'error', message: 'No assignments selected' }.to_json if request.xhr?
    end
  end
end
