require_relative 'base_controller'

class DiscussionController < BaseController
  get '/course/:id/discussions/:forum_id/topics/:topic_id' do
    @course_id = params[:id]
    @forum_id = params[:forum_id]
    @topic_id = params[:topic_id]
    @active_tab = 'discussions'
    
    @breadcrumb_trail = [
      { title: 'Discussions', url: "/course/#{@course_id}/discussions" },
      { title: 'Topic', url: "/course/#{@course_id}/discussions/#{@forum_id}/topics/#{@topic_id}" }
    ]
    erb :discussion_threads
  end

  get '/course/:id/discussions/:forum_id/topics' do
    @course_id = params[:id]
    @forum_id = params[:forum_id]
    @active_tab = 'discussions'
    
    erb :discussion_topics
  end

  post '/course/:id/discussions/toggle_collapse' do
    key = params[:section].present? ? "#{params[:topic_id]}:#{params[:section]}" : params[:topic_id]
    @user_prefs.toggle_topic_collapse(key)
    if request.xhr?
      { status: 'ok', collapsed: @user_prefs.topic_collapsed?(key) }.to_json
    else
      redirect back
    end
  end
end
