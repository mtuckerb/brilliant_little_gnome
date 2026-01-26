require_relative 'base_controller'

class AuthController < BaseController
  get '/setup' do
    @host = $client.host
    erb :setup
  end

  post '/setup' do
    host = params[:host].strip
    cookies = params[:cookies].strip
    $client.save_connection_config(host, cookies)
    whoami = $client.get_who_am_i
    if whoami && whoami['Identifier']
      redirect '/dashboard'
    else
      @error = "Could not authenticate with Brightspace."
      erb :setup
    end
  end

  get '/login' do
    redirect $client.auth_url
  end

  get '/callback' do
    if params['code'] && $client.exchange_code(params['code'])
      redirect '/dashboard'
    else
      "Authentication Failed. <a href='/'>Retry</a>"
    end
  end

  get '/local_login' do
    erb :login, layout: false
  end

  post '/local_login' do
    if @user_prefs.web_access_passcode.present? && params[:passcode] == @user_prefs.web_access_passcode
      session[:local_authenticated] = true
      redirect '/dashboard'
    else
      @error = "Invalid passcode"
      erb :login, layout: false
    end
  end

  get '/local_logout' do
    session[:local_authenticated] = false
    redirect '/local_login'
  end
end
