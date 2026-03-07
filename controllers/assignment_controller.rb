require_relative 'base_controller'

class AssignmentController < BaseController
  helpers CourseHelpers

  before '/course/:id/assignments/:assignment_id*' do
    @course_id = params[:id]
    @assignment_id = params[:assignment_id]
  end

  get '/course/:id/assignments/:assignment_id' do
    if @assignment_id.start_with?('syn_')
      rec = Assignment.find_by(brightspace_id: @assignment_id, course_id: @course_id)
      halt 404, "Task not found" unless rec
      @assignment = {
        'Id' => rec.brightspace_id, 'Name' => rec.name, 'DueDate' => rec.due_date&.iso8601,
        'Instructions' => { 'Text' => rec.description }, 'Synthetic' => true,
        'ExternalUrl' => rec.external_url, 'Completed' => rec.completed
      }
    else
      @assignment = $client.get_assignment(@course_id, @assignment_id)
    end
    
    halt 404, "Assignment not found" unless @assignment

    @feedback = $client.get_assignment_feedback(@course_id, @assignment_id) unless @assignment['Synthetic']
    @rubrics = $client.get_assignment_rubrics(@course_id, @assignment_id) unless @assignment['Synthetic']
    @submission_data = $client.get_assignment_submissions(@course_id, @assignment_id) unless @assignment['Synthetic']
    
    @breadcrumb_trail = [
      { title: 'Assignments', url: "/course/#{@course_id}/assignments" },
      { title: @assignment['Name'], url: "/course/#{@course_id}/assignments/#{@assignment_id}" }
    ]

    @feedback_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:feedback")
    @instructions_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:instructions")
    @submissions_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:submissions")
    @rubric_collapsed = @user_prefs.topic_collapsed?("assignment:#{@assignment_id}:rubric")

    @edit_mode = params[:edit] == 'true'
    
    if request.xhr? && request.accept?('application/json')
      content_type :json
      return @assignment.to_json
    end

    erb :assignment_detail
  end

  post '/course/:id/assignments/:assignment_id/update' do
    assignment = Assignment.find_by(brightspace_id: params[:assignment_id], course_id: params[:id])
    halt 404, "Task not found" unless assignment

    parsed_date = nil
    if params[:due_date].present?
      Time.use_zone(@user_prefs.time_zone || "UTC") do
        parsed_date = params[:due_time].present? ? Time.zone.parse("#{params[:due_date]} #{params[:due_time]}") : Time.zone.parse(params[:due_date]).end_of_day
      end
    end

    if assignment.update(
      name: params[:name], description: params[:description], due_date: parsed_date,
      external_url: params[:external_url], manually_edited: true, manually_edited_at: Time.current
    )
      if request.xhr?
        content_type :json
        { status: 'ok', message: "Task updated" }.to_json
      else
        flash[:success] = "Task updated"
        redirect "/course/#{params[:id]}/assignments/#{params[:assignment_id]}"
      end
    else
      if request.xhr?
        content_type :json
        status 422
        { status: 'error', error: assignment.errors.full_messages.join(", ") }.to_json
      else
        flash[:error] = assignment.errors.full_messages.join(", ")
        redirect back
      end
    end
  end

  post '/assignments/:id/toggle_complete' do
    assignment = Assignment.find(params[:id])
    assignment.update(completed: !assignment.completed, completed_at: (assignment.completed ? nil : Time.current))
    
    if request.xhr?
      { status: 'ok', completed: assignment.completed }.to_json
    else
      redirect back
    end
  end

  post '/assignments/:id/toggle_optional' do
    assignment = Assignment.find(params[:id])
    assignment.update(optional: !assignment.optional)
    
    if request.xhr?
      content_type :json
      { status: 'ok', optional: assignment.optional }.to_json
    else
      redirect back
    end
  end

  post '/assignments/:id/update_due_date' do
    assignment = Assignment.find(params[:id])
    new_date = params[:due_date]
    parsed_date = nil

    if new_date.present?
      tz_name = @user_prefs.time_zone || "UTC"
      Time.use_zone(tz_name) do
        parsed_date = new_date.length <= 10 ? Time.zone.parse(new_date).end_of_day : Time.zone.parse(new_date)
      end
    end

    assignment.update(due_date: parsed_date, manually_edited: true, manually_edited_at: Time.current)
    
    if request.xhr?
      content_type :json
      { status: 'ok', due_date: assignment.due_date ? assignment.due_date.iso8601 : nil }.to_json
    else
      redirect back
    end
  end

  post '/course/:id/assignments/:assignment_id/delete' do
    assignment = Assignment.find_by(brightspace_id: params[:assignment_id], course_id: params[:id])
    halt 404, "Task not found" unless assignment && assignment.synthetic

    assignment.destroy
    flash[:success] = "Task deleted"
    redirect "/course/#{params[:id]}/assignments"
  end

  # --- Synthetic / Manual Tasks ---

  post '/course/:id/synthetic_tasks' do
    # JSON request from JS popover
    data = JSON.parse(request.body.read) rescue {}
    course_id = params[:id]
    
    # Generate unique ID
    brightspace_id = "syn_#{SecureRandom.hex(8)}"
    
    parsed_date = nil
    if data['due_date'].present?
      Time.use_zone(@user_prefs.time_zone || "UTC") do
        parsed_date = Time.zone.parse(data['due_date']).end_of_day
      end
    else
      # Default to course end of week if available
      course = Course.find_by(org_unit_id: course_id)
      parsed_date = course.end_of_week_date(Time.current) if course.respond_to?(:end_of_week_date)
    end

    assignment = Assignment.create!(
      course_id: course_id,
      brightspace_id: brightspace_id,
      name: data['name'],
      description: data['description'],
      due_date: parsed_date,
      synthetic: true,
      manually_edited: true,
      manually_edited_at: Time.current
    )

    { status: 'ok', id: assignment.id, brightspace_id: brightspace_id }.to_json
  end

  post '/course/:id/module/:module_id/create_tasks' do
    # Multi-task creation from module view
    task_indices = params[:tasks] || []
    module_title = params[:module_title]
    
    added_count = 0
    task_indices.each do |idx|
      name = params["task_names_#{idx}"]
      type = params["task_types_#{idx}"]
      url = params["task_urls_#{idx}"]
      due_str = params["task_dates_#{idx}"]
      
      next if name.blank?

      # Check if already exists by name/url in this course
      existing = Assignment.find_by(course_id: params[:id], name: name, external_url: url)
      next if existing

      brightspace_id = "syn_#{SecureRandom.hex(8)}"
      
      parsed_date = nil
      if due_str.present?
        Time.use_zone(@user_prefs.time_zone || "UTC") do
          parsed_date = Time.zone.parse(due_str).end_of_day
        end
      end

      Assignment.create!(
        course_id: params[:id],
        brightspace_id: brightspace_id,
        name: name,
        assignment_type: type.downcase,
        external_url: url,
        due_date: parsed_date,
        description: "Synthesized from module: #{module_title}",
        synthetic: true
      )
      added_count += 1
    end

    flash[:success] = "Added #{added_count} task(s) to assignments"
    redirect "/course/#{params[:id]}/assignments"
  end

  post '/course/:id/announcements/:announcement_id/create_task' do
    create_task_from_announcement
  end

  get '/course/:id/announcements/:announcement_id/create_task' do
    create_task_from_announcement
  end

  private

  def create_task_from_announcement
    course_id = params[:id]
    announcement_id = params[:announcement_id]
    
    # Try to find the notification to get details
    notification = Notification.find_by(course_id: course_id, external_id: announcement_id)
    
    title = params[:title] || (notification ? notification.title : "New Task from Announcement")
    description = notification ? "From announcement: #{notification.title}\n\n#{notification.body}" : "Created from announcement #{announcement_id}"
    
    brightspace_id = "syn_#{SecureRandom.hex(8)}"
    
    # Calculate default due date (end of week)
    course = Course.find_by(org_unit_id: course_id)
    parsed_date = nil
    if course
      parsed_date = course.end_of_week_date(Time.current)
    else
      # Default to 7 days from now
      parsed_date = 7.days.from_now.end_of_day
    end

    Assignment.create!(
      course_id: course_id,
      brightspace_id: brightspace_id,
      name: title,
      description: description,
      due_date: parsed_date,
      synthetic: true,
      manually_edited: true,
      manually_edited_at: Time.current
    )

    if request.xhr?
      content_type :json
      { status: 'ok', brightspace_id: brightspace_id }.to_json
    else
      flash[:success] = "Created task from announcement"
      redirect "/course/#{course_id}/assignments/#{brightspace_id}"
    end
  end
end
