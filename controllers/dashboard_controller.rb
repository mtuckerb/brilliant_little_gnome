require_relative 'base_controller'

class DashboardController < BaseController
  get '/' do
    if configured?
      redirect '/dashboard'
    else
      erb :login
    end
  end

  get '/dashboard' do
    redirect '/' unless configured?
    @active_tab = 'dashboard'
    erb :dashboard
  end

  get '/calendar' do
    @active_tab = 'calendar'
    @view = params[:view] || 'week'
    tz_name = @user_prefs&.time_zone || "UTC"
    Time.zone = tz_name
    @date = params[:date] ? Date.parse(params[:date]) : Time.zone.today
    @show_completed = params[:show_completed] == 'true'
    erb :calendar
  end

  get '/notifications' do
    @active_tab = 'notifications'
    @courses = $client.get_enrollments
    @user = $client.get_who_am_i
    
    # Context breadcrumb
    context_id = params[:from_course] || params[:course_id]
    @context_course = Course.find_by(org_unit_id: context_id) if context_id
    
    # Sync in background
    Thread.new { ActiveRecord::Base.connection_pool.with_connection { $client.sync_notifications(@courses, @user) } }
    
    erb :notifications
  end

  post '/notifications/:id/mark_read' do
    notification = Notification.find(params[:id])
    notification.update(is_read: true)

    Thread.new do
      ext_id = notification.external_id
      if ext_id.start_with?("news_")
        parts = ext_id.split('_')
        $client.dismiss_news_item(parts[1], parts[2]) if parts.size >= 3
      elsif ext_id.match?(/^\d+$/)
        $client.mark_notification_read(ext_id)
      end
    end

    if request.xhr?
      content_type :json
      { status: 'ok', id: notification.id, is_read: true }.to_json
    else
      redirect back
    end
  end

  get '/notifications/:id/view' do
    notification = Notification.find(params[:id])
    notification.update(is_read: true)

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
    unread_notifications = Notification.where(is_read: false).select(:external_id, :course_id)
    Notification.update_all(is_read: true)

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

  post '/calendar/update_due_date' do
    type = params[:type]
    item_id = params[:id]
    new_date = params[:due_date]
    
    model = (type == 'assignment' ? Assignment : Grade)
    item = model.find_by(brightspace_id: item_id)
    
    if item
      begin
        if new_date.present?
          tz_name = @user_prefs.time_zone || "UTC"
          Time.use_zone(tz_name) do
            item.due_date = new_date.length <= 10 ? Time.zone.parse(new_date).end_of_day : Time.zone.parse(new_date)
          end
          if type == 'assignment'
            item.manually_edited = true
            item.manually_edited_at = Time.current
          end
          item.save!
        else
          item.due_date = nil
          if type == 'assignment'
            item.manually_edited = true
            item.manually_edited_at = Time.current
          end
          item.save!
        end
      rescue => e
        status 422
        return { status: 'error', message: e.message }.to_json if request.xhr?
        flash[:error] = "Error: #{e.message}"
      end
    else
      status 404
      return { status: 'error', message: "Item not found" }.to_json if request.xhr?
    end
    
    if request.xhr?
      content_type :json
      { status: 'ok', due_date: item&.due_date ? item.due_date.iso8601 : nil }.to_json
    else
      redirect back
    end
  end

  get '/settings' do
    @active_tab = 'settings'
    @host = $client.host
    erb :settings
  end

  post '/settings' do
    if params[:host].present? && params[:cookies].present?
      $client.save_connection_config(params[:host].strip, params[:cookies].strip)
    end

    updates = {
      display_name: params[:display_name],
      time_zone: params[:time_zone],
      api_enabled: params[:api_enabled] == 'true',
      api_key: params[:api_key],
      web_access_passcode: params[:web_access_passcode],
      show_course_list: params[:show_course_list] == 'true',
      show_upcoming_assignments: params[:show_upcoming_assignments] == 'true',
      show_recent_updates: params[:show_recent_updates] == 'true'
    }

    updates[:jwt_secret] = params[:jwt_secret] if params[:jwt_secret].present?

    @user_prefs.update(updates)
    redirect '/settings'
  end

  post '/settings/semester_colors' do
    colors = JSON.parse(params[:colors]) rescue {}
    @user_prefs.update(semester_colors: colors)
    if request.xhr?
      content_type :json
      { status: 'ok' }.to_json
    else
      redirect '/settings'
    end
  end

  get '/archive' do
    redirect '/' unless configured?
    @active_tab = 'archive'
    @courses = Course.all.order(is_pinned: :desc, last_accessed_at: :desc)
    @all_semesters = Course.where.not(semester: [nil, ""]).pluck(:semester).uniq.sort
    erb :archive
  end

  get '/health' do
    status 200
    "OK"
  end
end
