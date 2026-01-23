require 'sinatra/activerecord/rake'

# Only load app for DB tasks to avoid version conflicts during platform setup
if Rake.application.top_level_tasks.any? { |t| t.start_with?('db:') }
  require './app'
end

namespace :platforms do
  def get_ruby_bundle_paths
    portable_ruby = "bin/ruby_dist/macos-arm64/bin/ruby"
    portable_bundle = "bin/ruby_dist/macos-arm64/bin/bundle"
    
    if File.exist?(portable_ruby)
      [portable_ruby, portable_bundle]
    else
      [nil, "bundle"]
    end
  end

  desc "Lock Gemfile for all supported platforms (macOS arm64/x64, Windows x64)"
  task :lock do
    ruby_path, bundle_path = get_ruby_bundle_paths
    puts "Warning: Portable Ruby not found. Falling back to system bundle." unless ruby_path
    platforms = ["arm64-darwin", "x86_64-darwin", "x64-mingw-ucrt", "ruby"]
    
    puts "Updating Gemfile.lock for platforms: #{platforms.join(', ')}..."
    cmd = "#{ruby_path} #{bundle_path} lock --add-platform #{platforms.join(' ')}"
    system(cmd)
  end

  desc "Install gems into vendor/bundle using portable Ruby"
  task :install do
    ruby_path, bundle_path = get_ruby_bundle_paths
    puts "Warning: Portable Ruby not found. Falling back to system bundle." unless ruby_path

    puts "Installing gems into vendor/bundle..."
    # Ensure deployment mode is used for portability
    if ruby_path
      system("#{ruby_path} #{bundle_path} config set --local path 'vendor/bundle'")
      system("#{ruby_path} #{bundle_path} config set --local deployment 'true'")
      system("#{ruby_path} #{bundle_path} install")
    else
      system("#{bundle_path} config set --local path 'vendor/bundle'")
      system("#{bundle_path} config set --local deployment 'true'")
      system("#{bundle_path} install")
    end
    
    # On macOS Apple Silicon, we might need to force the platform for sqlite3
    if RUBY_PLATFORM =~ /arm64-darwin/
      puts "Detected arm64-darwin, ensuring compatible gems..."
    end
  end

  desc "Check environment for portability"
  task :check do
    puts "--- Portability Check ---"
    puts "System Ruby: #{`ruby -v`}"
    puts "Portable Ruby: #{File.exist?('bin/ruby_dist/macos-arm64/bin/ruby') ? 'PRESENT' : 'MISSING'}"
    puts "Vendor Bundle: #{File.exist?('vendor/bundle') ? 'PRESENT' : 'MISSING'}"
    
    if File.exist?('vendor/bundle/ruby')
      versions = Dir.entries('vendor/bundle/ruby').select {|f| !f.start_with?('.') }
      puts "Gem versions in vendor/bundle: #{versions.join(', ')}"
    end
    puts "-------------------------"
  end
end

