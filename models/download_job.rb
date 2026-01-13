require 'securerandom'

class DownloadJob
  attr_reader :id, :status, :total_files, :completed_files, :zip_path, :error

  @jobs = {}
  @mutex = Mutex.new

  def self.create(course_id, files, client)
    job = new(course_id, files, client)
    @mutex.synchronize { @jobs[job.id] = job }
    job.start
    job
  end

  def self.find(id)
    @mutex.synchronize { @jobs[id] }
  end

  def initialize(course_id, files, client)
    @id = SecureRandom.uuid
    @course_id = course_id
    @files = files
    @client = client
    @status = :preparing
    @total_files = files.size
    @completed_files = 0
    @zip_path = nil
    @error = nil
  end

  def start
    Thread.new do
      begin
        @status = :downloading
        temp_dir = Dir.mktmpdir("bs_download_#{@id}")
        
        @zip_path = File.join(Dir.tmpdir, "Britespace_#{@course_id}_#{@id}.zip")

        Zip::File.open(@zip_path, Zip::File::CREATE) do |zipfile|
          @files.each do |f|
            resp = @client.download_file(f[:path])
            if resp && resp.code == '200'
              safe_name = f[:title].gsub(/[^0-9A-Za-z.\- ]/, '_')
              safe_name += ".pdf" unless safe_name.include?('.')
              
              # Construct path within zip using the folder metadata
              zip_entry_path = if f[:folder]
                                 "#{f[:folder]}/#{safe_name}"
                               else
                                 safe_name
                               end

              # Handle duplicate names in zip
              original_full_path = zip_entry_path
              counter = 1
              while zipfile.find_entry(zip_entry_path)
                ext = File.extname(safe_name)
                base = File.basename(safe_name, ext)
                new_filename = "#{base}_#{counter}#{ext}"
                zip_entry_path = if f[:folder]
                                   "#{f[:folder]}/#{new_filename}"
                                 else
                                   new_filename
                                 end
                counter += 1
              end

              zipfile.get_output_stream(zip_entry_path) { |os| os.write resp.body }
            end
            @completed_files += 1
            sleep(0.5) # Respectful rate limit
          end
        end

        @status = :completed
      rescue => e
        @status = :failed
        @error = e.message
        puts "Job #{@id} failed: #{e.message}"
        puts e.backtrace
      end
    end
  end

  def progress
    return 0 if @total_files == 0
    ((@completed_files.to_f / @total_files) * 100).round
  end
end
