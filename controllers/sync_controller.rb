require_relative 'base_controller'

class SyncController < BaseController
  get '/sync/status' do
    content_type :json
    {
      status: $client.sync_status[:status],
      progress: $client.sync_status[:progress],
      current_task: $client.sync_status[:current_task],
      degraded_mode: $client.degraded_mode,
      last_auth_error: $client.last_auth_error
    }.to_json
  end

  post '/sync/force' do
    UserPreference.set('force_full_sync', 'true')
    $client.sync_all_courses_proactively
    redirect '/dashboard'
  end

  get '/job/:id' do
    @job = DownloadJob.find(params[:id])
    erb :job_status
  end

  get '/job/:id/status' do
    job = DownloadJob.find(params[:id])
    if job
      content_type :json
      { status: job.status, progress: job.progress, total: job.total_files }.to_json
    else
      status 404
    end
  end

  get '/job/:id/download' do
    job = DownloadJob.find(params[:id])
    if job && job.status == :completed && File.exist?(job.zip_path)
      send_file job.zip_path, type: 'application/zip', filename: job.download_filename
    else
      "Not ready."
    end
  end
end
