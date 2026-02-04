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
    $client.degraded_mode = false # Explicitly reset before testing
    whoami = $client.get_who_am_i
    if whoami && whoami['Identifier']
      redirect '/dashboard'
    else
      @error = "Could not authenticate with Brightspace."
      erb :setup
    end
  end

  get '/auth/magic' do
    @host = params[:host] || $client.host || "courses.maine.edu"
    erb :browser_login_prompt
  end

  post '/auth/magic' do
    host = params[:host] || $client.host || "courses.maine.edu"
    # Use the existing helper to capture cookies via a non-headless browser
    cookies = BrilliantAuthHelper.fetch_cookies(host)
    if cookies
      $client.save_connection_config(host, cookies)
      
      # Verify the session before redirecting to ensure we don't end up in a degraded mode loop
      $client.degraded_mode = false
      whoami = $client.get_who_am_i
      if whoami && whoami['Identifier']
        flash[:success] = "Successfully authenticated!"
        redirect '/dashboard'
      else
        @error = "Login captured cookies, but they failed to authenticate with the API. Please try again."
        erb :setup
      end
    else
      @error = "Magic Login failed or timed out."
      erb :setup
    end
  end

  get '/callback' do
    if params['code'] && $client.exchange_code(params['code'])
      redirect '/dashboard'
    else
      "Authentication Failed. <a href='/'>Retry</a>"
    end
  end
end
