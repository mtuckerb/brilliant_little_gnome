require 'kramdown'

module CourseHelpers
  def truncate_text(text, max_length = 20)
    return text if text.nil? || text.length <= max_length
    text[0...max_length-1] + "…"
  end

  def author_name(obj)
    return "Anonymous" unless obj
    
    return obj.author_name if obj.respond_to?(:author_name) && obj.author_name

    if obj['Author']
      obj['Author']['DisplayName']
    elsif obj['PostingUserDisplayName']
      obj['PostingUserDisplayName']
    else
      "Anonymous"
    end
  end

  def is_instructor?(obj)
    return false unless obj && obj['Author']
    obj['Author']['IsInstructor'] == true || obj['Author']['RoleName'] =~ /Instructor/i
  rescue
    false
  end

  def render_markdown(text)
    Brilliant::TextProcessor.render_markdown(text)
  end

  def render_markdown_inline(text)
    return "" if text.nil? || text.empty?
    html = render_markdown(text).strip
    
    # Remove surrounding <p> tags if they exist
    if html.start_with?("<p>") && html.end_with?("</p>")
      html = html[3..-5]
    end
    html
  end

  def fix_links(html)
    Brilliant::TextProcessor.fix_links(html, $client&.host)
  end

  # Normalize a URL to point to Brightspace, not the local Brilliant server.
  # Handles: relative paths (/d2l/...), full local URLs (http://10.1.0.31:4567/d2l/...), and correct URLs.
  def normalize_brightspace_url(url)
    return nil if url.nil? || url.to_s.strip.empty?
    bs_host = brightspace_link_host
    normalized = url.to_s.strip
    # Replace any local/wrong host prefix in full URLs with the Brightspace host
    normalized = normalized.sub(%r{\Ahttps?://(?!#{Regexp.escape(bs_host)})[\w.:-]+(?=/d2l|/content|/le/)}, "https://#{bs_host}")
    # If it's a relative path, prepend the Brightspace host
    normalized = "https://#{bs_host}#{normalized}" if normalized.start_with?('/')
    normalized
  end

  def brightspace_link_host
    preferred = @user_prefs&.brightspace_host.presence || UserPreference.current&.brightspace_host.presence
    preferred = sanitize_host(preferred)
    return preferred if preferred && !local_or_private_host?(preferred)

    client_host = sanitize_host($client&.host)
    return client_host if client_host && !local_or_private_host?(client_host)

    'courses.maine.edu'
  rescue
    'courses.maine.edu'
  end

  def sanitize_host(host)
    return nil if host.nil? || host.to_s.strip.empty?
    host.to_s.strip.gsub(%r{\Ahttps?://}i, '').split('/').first
  end

  def local_or_private_host?(host)
    bare = host.to_s.downcase.split(':').first
    return true if bare == 'localhost' || bare.end_with?('.local')

    return false unless bare.match?(/\A\d{1,3}(?:\.\d{1,3}){3}\z/)
    octets = bare.split('.').map(&:to_i)
    return true if octets[0] == 10
    return true if octets[0] == 127
    return true if octets[0] == 192 && octets[1] == 168
    return true if octets[0] == 172 && (16..31).cover?(octets[1])

    false
  end

  def html_to_markdown(html)
    Brilliant::TextProcessor.html_to_markdown(html)
  end

  def clean_notification_title(title, course_name)
    return title if title.nil? || course_name.nil?
    
    cleaned = title.dup
    
    # 1. Remove the full course name if it appears in the title
    # We escape it because course names can contain parentheses
    cleaned.gsub!(course_name, '')
    
    # 2. If it's still containing redundant parts, try more targeted removal
    # Remove things like "ABC 123:0001-" or "ABC 123:0001 "
    section_match = course_name.match(/^(\w{3}\s\d{3}:\d{4})[- ]/i)
    if section_match
      cleaned.gsub!(section_match[1], '')
    end

    # Remove the semester part if it exists
    info = extract_course_info(course_name)
    if info[:semester]
      cleaned.gsub!(/\(#{Regexp.escape(info[:semester])}\)/, '')
      cleaned.gsub!(info[:semester], '')
    end

    cleaned.gsub!(/\(Online\)/i, '')
    
    # Cleanup extra spaces and colons that might be left over
    cleaned = cleaned.gsub(/\s+/, ' ').strip
    
    # If we removed everything or left only a colon/dash, restore original part but cleaner
    return title if cleaned.empty? || cleaned == ":" || cleaned == "-"
    
    cleaned.gsub(/[:\s-]+$/, '')
  end

  def render_content(text)
    fix_links(render_markdown(text))
  end


  def find_module(modules, id)
    return nil unless modules
    modules.each do |m|
      return m if m['ModuleId'].to_s == id.to_s
      found = find_module(m['Modules'], id)
      return found if found
    end
    nil
  end

  def find_topic(modules, id)
    return nil unless modules
    modules.each do |m|
      if m['Topics']
        topic = m['Topics'].find { |t| (t['Id'] || t['TopicId'] || t['Identifier']).to_s == id.to_s }
        return topic if topic
      end
      found = find_topic(m['Modules'], id)
      return found if found
    end
    nil
  end


  def find_syllabus_items(modules, items = { modules: [], topics: [] })
    return items unless modules
    modules.each do |m|
      if m['Title'].downcase.match?(/syllabus|course overview|getting started|start here/)
        items[:modules] << m
      end
      
      (m['Topics'] || []).each do |t|
        if t['Title'].downcase.match?(/syllabus|course overview/)
          items[:topics] << t
        end
      end
      
      find_syllabus_items(m['Modules'], items) if m['Modules']
    end
    items
  end

  # Builds breadcrumbs from TOC for a given module ID
  def build_breadcrumbs(modules, target_id, path = [])
    return nil unless modules
    modules.each do |m|
      current_path = path + [{ id: m['ModuleId'], title: m['Title'] }]
      return current_path if m['ModuleId'].to_s == target_id.to_s
      
      found = build_breadcrumbs(m['Modules'], target_id, current_path)
      return found if found
    end
    nil
  end

  # Recursively collects all files from a module tree
  def collect_all_files(course_id, module_obj, folder_name, files = [])
    return files unless module_obj

    # Add topics that are files
    if module_obj['Topics']
      module_obj['Topics'].each do |t|
        # Unique ID for the topic (Identifier is most stable in our normalized DB)
        tid = t['Id'] || t['TopicId'] || t['Identifier']
        url = t['Url']
        
        next unless tid && url

        # 1. External Links / LTI (Handle as .url shortcut)
        if t['Type'].to_s == "3" || t['Type'].to_s == "Link"
          safe_title = (t['Title'] || "Link").gsub(/[^0-9a-z]/i, '_')
          files << {
            title: "#{safe_title}.url",
            content: "[InternetShortcut]\r\nURL=#{url}\r\n",
            folder: folder_name
          }
        else
          # 2. Binary content (PDF, Word, etc.)
          # We use the le API topic file endpoint
          files << { 
            id: tid, 
            title: t['Title'], 
            path: "/d2l/api/le/1.40/#{course_id}/content/topics/#{tid}/file",
            folder: folder_name
          }
        end
      end
    end

    # Recurse into sub-modules
    if module_obj['Modules']
      module_obj['Modules'].each do |sub|
        # Create subfolder path
        safe_sub_title = (sub['Title'] || "Untitled").gsub(/[^0-9a-z]/i, '_')
        sub_folder = "#{folder_name}/#{safe_sub_title}"
        collect_all_files(course_id, sub, sub_folder, files)
      end
    end

    files
  end

  # Collects all files with the requested structure
  def collect_everything(course_id, client, toc)
    files = []
    
    # 1. Syllabus/Overview
    overview = client.get_overview(course_id)
    if overview
      # Main attachment (The one appearing in the header)
      files << {
        title: "Syllabus_Overview", # DownloadJob will append correct extension
        path: "/d2l/api/le/1.40/#{course_id}/overview/attachment",
        folder: "Syllabus_Overview"
      }
      
      # Additional attachments (archived structure)
      if overview['Attachments']
        overview['Attachments'].each do |att|
          files << {
            title: att['FileName'],
            path: "/d2l/api/le/1.40/#{course_id}/overview/attachments/#{att['FileId'] || att['Id']}",
            folder: "Syllabus_Overview"
          }
        end
      end
      if overview['LinkAttachments']
        overview['LinkAttachments'].each do |link|
          safe_link_name = (link['Title'] || link['LinkName'] || "Link").gsub(/[^0-9a-z]/i, '_')
          files << {
            title: "#{safe_link_name}.url",
            content: "[InternetShortcut]\r\nURL=#{link['Url'] || link['Href']}\r\n",
            folder: "Syllabus_Overview"
          }
        end
      end
    end

    # 2. Announcements (only if they have attachments)
    news = client.get_news(course_id) || []
    news.each do |item|
      if item['Attachments'] && !item['Attachments'].empty?
        item['Attachments'].each do |att|
          files << {
            title: att['FileName'],
            path: "/d2l/api/le/1.40/#{course_id}/news/#{item['Id']}/attachments/#{att['FileId']}",
            folder: "Announcements"
          }
        end
      end
    end

    # 3. Assignments (with attachments)
    assignments_list = client.get_assignments(course_id) || []
    assignments_list.each do |a_summary|
      # Fetch detail for each assignment to ensure we get attachments 
      # (The list view often excludes them)
      a = client.get_assignment(course_id, a_summary['Id']) || a_summary
      
      if a['Attachments'] && !a['Attachments'].empty?
        a['Attachments'].each do |att|
          files << {
            title: att['FileName'],
            path: "/d2l/api/le/1.40/#{course_id}/dropbox/folders/#{a['Id']}/attachments/#{att['FileId']}",
            folder: "Assignments/#{a['Name'].gsub(/[^0-9a-z]/i, '_')}"
          }
        end
      end

      if a['LinkAttachments'] && !a['LinkAttachments'].empty?
        a['LinkAttachments'].each do |link|
          safe_link_name = link['LinkName'].gsub(/[^0-9a-z]/i, '_')
          files << {
            title: "#{safe_link_name}.url",
            content: "[InternetShortcut]\r\nURL=#{link['Href']}\r\n",
            folder: "Assignments/#{a['Name'].gsub(/[^0-9a-z]/i, '_')}"
          }
        end
      end

      # Feedback
      fb = client.get_assignment_feedback(course_id, a_summary['Id'])
      if fb
        folder = "Assignments/#{a['Name'].gsub(/[^0-9a-z]/i, '_')}/Feedback"
        if fb['Attachments'] && !fb['Attachments'].empty?
          fb['Attachments'].each do |att|
            files << {
              title: att['FileName'],
              path: "/d2l/api/le/1.40/#{course_id}/dropbox/folders/#{a['Id']}/feedback/attachments/#{att['FileId']}",
              folder: folder
            }
          end
        end
        if fb['LinkAttachments'] && !fb['LinkAttachments'].empty?
          fb['LinkAttachments'].each do |link|
            safe_link_name = (link['Title'] || link['LinkName'] || "Link").gsub(/[^0-9a-z]/i, '_')
            files << {
              title: "#{safe_link_name}.url",
              content: "[InternetShortcut]\r\nURL=#{link['Url'] || link['Href']}\r\n",
              folder: folder
            }
          end
        end
      end
    end

    # 4. Table of Contents (Modules)
    if toc && toc['Modules']
      toc['Modules'].each do |m|
        folder_name = "Table_of_Contents/#{m['Title'].gsub(/[^0-9a-z]/i, '_')}"
        collect_all_files(course_id, m, folder_name, files)
      end
    end

    files
  end

  # Returns the list of parent module IDs for a given module ID
  def find_lineage(modules, target_id, path = [])
    return nil unless modules
    modules.each do |m|
      current_path = path + [m['ModuleId'].to_s]
      return current_path if m['ModuleId'].to_s == target_id.to_s
      
      found = find_lineage(m['Modules'], target_id, current_path)
      return found if found
    end
    nil
  end

  def extract_course_info(full_name, org_unit_id = nil)
    self.class.extract_course_info(full_name, org_unit_id)
  end

  def self.extract_course_info(full_name, org_unit_id = nil)
    return { course_display: "", short_name: "", prefix: "", is_online: false, semester: nil, pill_style: "" } if full_name.to_s.empty?

    # Sanity check: if it's just a number, it's not a full name
    if full_name.to_s.match?(/^\d+$/)
      org_unit_id ||= full_name.to_s
      course = Course.find_by(org_unit_id: org_unit_id)
      full_name = course.name if course && course.name.present? && !course.name.to_s.match?(/^\d+$/)
    end

    # Pattern: SWO 370:0001-Human Behav (Online) (2026 Spring)
    regex = /^(\w{3}\s\d{3}):\d{4}-(.*?)\s*(\(Online\))?\s*(\(\d{4}\s+(?:Spring|Fall|Summer|Winter)\))$/i
    match = full_name.match(regex)
    
    if match
      short_name = match[2].strip
      short_name = match[1] if short_name.empty?
      prefix = match[1].strip
      semester = match[4].to_s.gsub(/[()]/, '').strip
      
      {
        course_display: "#{match[1]} - #{match[2].strip}",
        short_name: short_name,
        prefix: prefix,
        is_online: !match[3].to_s.empty?,
        semester: semester,
        pill_style: CourseHelpers.course_pill_style(full_name, semester, org_unit_id)
      }
    else 
      # Fallback logic
      is_online = full_name.downcase.include?('online')
      semester_match = full_name.match(/(\(?(?:\d{4}\s+(?:Spring|Fall|Summer|Winter))\)?)/i)
      semester = semester_match ? semester_match[1].gsub(/[()]/, '').strip : nil
      
      course_display = full_name
      prefix = full_name[0..6].strip
      if semester
        course_display = course_display.gsub(/\(#{Regexp.escape(semester)}\)/, '').gsub(semester, '').strip
      end
      course_display = course_display.gsub('(Online)', '').strip
      
      # For short_name, try to strip common prefixes like "ABC 123:0001-" or "ABC 123:"
      short_name = course_display.gsub(/^\w{3}\s\d{3}:\d{4}-/, '').gsub(/^\w{3}\s\d{3}:/, '').strip
      
      short_name = course_display if short_name.empty?

      {
        course_display: course_display,
        short_name: short_name,
        prefix: prefix,
        is_online: is_online,
        semester: semester,
        pill_style: CourseHelpers.course_pill_style(full_name, semester, org_unit_id)
      }
    end
  end

  # NEW: Search TOC for topics or modules matching a query
  def search_toc(modules, query, results = { modules: [], topics: [] })
    return results unless modules && query && !query.empty?
    
    q = query.downcase
    modules.each do |m|
      if m['Title'].to_s.downcase.include?(q)
        results[:modules] << { id: m['ModuleId'], title: m['Title'] }
      end
      
      if m['Topics']
        m['Topics'].each do |t|
          if t['Title'].to_s.downcase.include?(q)
            results[:topics] << { 
              id: (t['Id'] || t['TopicId']), 
              title: t['Title'], 
              module_id: m['ModuleId'],
              type: t['Type'],
              is_downloadable: (t['Url'] && t['Url'].start_with?('/content/enforced/'))
            }
          end
        end
      end
      
      search_toc(m['Modules'], query, results) if m['Modules']
    end
    results
  end

  # NEW: Global Search across all content
  def global_search(query)
    q = "%#{query.downcase}%"
    results = []

    # 1. Courses
    ::Course.where("lower(name) LIKE ? OR lower(code) LIKE ?", q, q).limit(5).each do |c|
      results << { type: 'course', title: c.name, url: "/course/#{c.org_unit_id}", subtitle: c.code, icon: 'fas fa-graduation-cap' }
    end

    # 2. Assignments
    ::Assignment.includes(:course).where("lower(assignments.name) LIKE ? OR lower(description) LIKE ?", q, q).limit(10).each do |a|
      results << { type: 'assignment', title: a.name, url: "/course/#{a.course_id}/assignments/#{a.brightspace_id}", subtitle: a.course&.name, icon: 'fas fa-tasks' }
    end

    # 3. Content Items (Files/Modules)
    ::ContentItem.where("lower(title) LIKE ?", q).limit(10).each do |i|
      results << { type: 'content', title: i.title, url: "/course/#{i.content_module&.course_id}/module/#{i.module_id}", subtitle: "Module: #{i.content_module&.title}", icon: 'fas fa-file-alt' }
    end

    # 4. Discussion Posts
    ::DiscussionPost.where("lower(subject) LIKE ? OR lower(body) LIKE ? OR lower(author_name) LIKE ?", q, q, q).limit(10).each do |p|
      topic = p.discussion_topic
      results << { type: 'discussion', title: p.subject, url: "/course/#{topic&.course_id}/discussions/#{topic&.forum_id}/topics/#{p.topic_id}", subtitle: "By #{p.author_name} in #{topic&.name}", icon: 'fas fa-comment-alt' }
    end

    # 5. Notifications
    ::Notification.where("lower(title) LIKE ? OR lower(body) LIKE ?", q, q).limit(10).each do |n|
      results << { type: 'notification', title: n.title, url: "/notifications/#{n.id}/view", subtitle: n.course_name, icon: 'fas fa-bell' }
    end

    results
  end

  # Builds a nested tree from a flat list of discussion posts
  def build_post_tree(posts)
    return [] unless posts && posts.any?
    
    # Map by ID for quick lookup
    post_map = {}
    posts.each do |p|
      p['Replies'] = []
      post_map[p['PostId'].to_s] = p
    end
    
    tree = []
    posts.each do |p|
      parent_id = p['ParentPostId']
      if parent_id && post_map[parent_id.to_s]
        post_map[parent_id.to_s]['Replies'] << p
      else
        tree << p
      end
    end
    
    # Sort top level by date (oldest first is typical for threads)
    tree.sort_by { |p| p['DatePosted'] || "" }
  end

  # NEW: Build a nested TOC tree from ActiveRecord objects
  def build_toc_tree(course_id)
    all_modules = ContentModule.where(course_id: course_id.to_s).order(sort_order: :asc)
    all_items = ContentItem.joins(:content_module).where(content_modules: { course_id: course_id.to_s }).order(sort_order: :asc)
    
    # Map items to modules
    items_by_module = all_items.group_by(&:module_id)
    
    # Build tree structure mirroring the API response format for view compatibility
    modules_map = {}
    all_modules.each do |m|
      modules_map[m.brightspace_id] = {
        'ModuleId' => m.brightspace_id,
        'Title' => m.title,
        'Description' => { 'Text' => m.description },
        'Modules' => [],
        'Topics' => (items_by_module[m.brightspace_id] || []).map do |item|
          {
            'Identifier' => item.brightspace_id,
            'Id' => item.brightspace_id,
            'TopicId' => item.brightspace_id,
            'Title' => item.title,
            'Type' => item.item_type,
            'Url' => item.url,
            'IsHidden' => item.is_hidden
          }
        end
      }
    end
    
    tree = []
    all_modules.each do |m|
      if m.parent_id && modules_map[m.parent_id]
        modules_map[m.parent_id]['Modules'] << modules_map[m.brightspace_id]
      else
        tree << modules_map[m.brightspace_id]
      end
    end
    
    { 'Modules' => tree }
  end

  # Grade Analytics Helpers
  def calculate_grade_stats(course_id)
    stats = Grade.calculate_weighted_total(course_id)
    stats || { score: 0, confidence: 0, total_weight_graded: 0, total_weight_possible: 0 }
  rescue => e
    puts "Error calculating grade stats: #{e.message}"
    { score: 0, confidence: 0, total_weight_graded: 0, total_weight_possible: 0 }
  end

  def confidence_color(confidence)
    return "has-text-grey-light" if confidence.nil?
    return "has-text-danger" if confidence < 30
    return "has-text-warning-dark" if confidence < 70
    "has-text-primary"
  end

  def grade_color(score)
    return "is-light" if score.nil?
    return "is-danger" if score < 70
    return "is-warning" if score < 90
    "is-primary"
  end

  def semester_weight(sem)
    return 0 unless sem
    year_match = sem.match(/\d{4}/)
    return 0 unless year_match
    year = year_match[0].to_i
    
    season_weight = 0
    if sem =~ /Winter/i
      season_weight = 1
    elsif sem =~ /Spring/i
      season_weight = 2
    elsif sem =~ /Summer/i
      season_weight = 3
    elsif sem =~ /Fall/i
      season_weight = 4
    end
    
    (year * 10) + season_weight
  end

  def synthesize_tasks(module_obj, course_name = "", tasks = [], parent_date = nil)
    return tasks unless module_obj
    
    module_title = module_obj['Title'] || ""
    
    # Try to extract a date from the module title (e.g. "Week 1 - 1/19" or "Week 1, January 26")
    inferred_date = nil
    year = Time.zone.now.year
    
    # Pattern 1: M/D (1/19)
    date_match = module_title.match(/(\d{1,2}\/\d{1,2})/) || module_title.match(/(\d{1,2}-\d{1,2})/)
    if date_match
      begin
        month, day = date_match[1].split(/[\/-]/).map(&:to_i)
        inferred_date = Time.zone.local(year, month, day, 23, 59, 59)
      rescue; end
    elsif (month_match = module_title.match(/(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\.?\s*,?\s*(\d{1,2})/i))
      # Pattern 2: Month Day (January 26 or Jan. 26 or January, 26)
      begin
        parsed = Time.zone.parse("#{month_match[1]} #{month_match[2]}")
        inferred_date = Time.zone.local(parsed.year, parsed.month, parsed.day, 23, 59, 59)
      rescue; end
    elsif (end_match = module_title.match(/Ending\s+(?:on\s+)?(\d{1,2}\s+[A-Za-z]+|[A-Za-z]+\s+\d{1,2})/i))
      begin
        parsed = Time.zone.parse(end_match[1])
        inferred_date = Time.zone.local(parsed.year, parsed.month, parsed.day, 23, 59, 59)
      rescue; end
    elsif (week_match = module_title.match(/Week\s*(\d+)/i))
       # If we just have a week number, but no date, maybe we can find a date in descriptions?
       # For now, stay with nil/parent
    end
    
    inferred_date ||= parent_date
    
    # Apply course end-of-week setting to the inferred date if it was just a raw date from a title
    if inferred_date && @course.respond_to?(:end_of_week_date)
      inferred_date = @course.end_of_week_date(inferred_date)
    elsif inferred_date.nil? && @course.respond_to?(:end_of_week_date)
      inferred_date = @course.end_of_week_date(Time.zone.now)
    end

    # 1. PROCESS TOPICS (Structural Identifying)
    (module_obj['Topics'] || []).each do |topic|
      is_task = false
      type_label = "Task"
      t_type = topic['Type'].to_s
      
      if t_type == "3" || t_type == "Link"
        is_task = true
        type_label = "Link"
        url = topic['Url'].to_s
        if url.include?('type=discuss')
          type_label = "Discussion"
        elsif url.include?('type=lti') || url.include?('rCode=')
          type_label = "External"
        elsif url.include?('type=quiz')
          type_label = "Quiz"
        end
      end

      if is_task
        tasks << {
          name: topic['Title'],
          type: type_label,
          due_date: inferred_date,
          source: 'topic',
          id: topic['Id'] || topic['TopicId'],
          module_id: module_obj['ModuleId'],
          context: module_title,
          url: topic['Url'],
          synthetic: true,
          external_url: topic['Url']
        }
      end
    end

    # 2. PROCESS DESCRIPTION
    desc = module_obj['Description']
    desc_text = ""
    if desc.is_a?(Hash)
      desc_text = (desc['Html'] || desc['Text'] || "").to_s
    else 
      desc_text = desc.to_s
    end
    
    unless desc_text.strip.empty?
      # Preserve anchor tags by converting them to a format that keeps the URL
      processed_desc = desc_text.gsub(/<a\s+(?:[^>]*?\s+)?href="([^"]*)"[^>]*>(.*?)<\/a>/i, '\2 (\1)')
      text = processed_desc.gsub(/<br\s*\/?>/i, "\n")
                           .gsub(/<\/p>/i, "\n")
                           .gsub(/<li[^>]*>/i, "\n- ")
                           .gsub(/<\/li>/i, "\n")
                           .gsub(/<[^>]+>/, ' ')
      lines = text.split("\n").map(&:strip).reject(&:empty?)
      
      current_category = "Task"
      lines.each do |line|
        if line =~ /^[A-Z\s]{4,}:?$/ || line =~ /^(Readings|Assignments|Tasks|To Do|Review Questions|Activities|Objectives|Watch):?$/i
          current_category = line.gsub(':', '').strip
          next
        end

        next if current_category =~ /Review Questions|Objectives|Resources/i

        content = nil
        content = nil
        if line =~ /^[ \t]*[\-•*]\s*(.*+)/ || line =~ /^[ \t]*\d+[\.\)]\s*(.*+)/
          content = ($1 || $2 || line).strip
        elsif current_category != "Task" && line.split.size >= 2 && line.split.size <= 8
          content = line
        elsif current_category == "Assignments" || current_category == "Readings"
          # If no bullet, but in a known list section, take the first sentence or short line
          content = line if line.length > 5 && line.length < 200
        end

        if content && content.length > 2
          next if tasks.any? { |t| t[:name].downcase == content.strip.downcase }
          
          # Try to find a specific internal date in the line (e.g. "due 02/04/26")
          line_date = nil
          if (item_date_match = line.match(/(\d{1,2}\/\d{1,2}(?:\/\d{2,4})?)/))
            begin
              line_date = Time.zone.parse(item_date_match[1])
              line_date = Time.zone.local(line_date.year, line_date.month, line_date.day, 23, 59, 59)
            rescue; end
          end

          # Extract first URL found in the line if any
          extracted_url = line.match(%r{https?://[^\s<"']+[^\s<"'. ,?!:)]})&.to_s
          extracted_url ||= line.match(/\(((\/[^\s<")]+))\)/)&.captures&.first

          # Clean task name: Remove the (URL) suffix if it exists to keep UI clean
          clean_task_name = content.strip.gsub(/\s*\(https?:\/\/[^\)]+\)$/, '')

          tasks << { 
            name: clean_task_name, 
            type: current_category,
            due_date: line_date || inferred_date,
            source: 'text',
            module_id: module_obj['ModuleId'],
            context: module_title,
            synthetic: true,
            external_url: extracted_url || "/d2l/le/content/#{@course_id}/viewContent/#{module_obj['ModuleId']}/View"
          }
        end
      end
    end

    # Deduplicate before recursion to keep tasks at their most specific level
    # but carry the date context down.
    # Actually, deduplication happens at the very end.

    # Update current tasks that don't have a date if we just found one
    tasks.each { |t| t[:due_date] ||= inferred_date if t[:context] == module_title }

    # 3. RECURSE INTO SUBMODULES
    (module_obj['Modules'] || []).each do |sub|
      synthesize_tasks(sub, course_name, tasks, inferred_date)
    end
    
    tasks.uniq { |t| t[:name] }
  end
  def self.course_pill_style(course_name, semester = nil, course_id = nil)
    return "" if course_name.nil? && course_id.nil?
    
    # 1. Check if we have a custom color saved for this course
    course = nil
    if course_id
      # We use CourseHelpers to avoid constant lookup issues if possible, but Course is a model
      course = ::Course.find_by(org_unit_id: course_id.to_s)
    end
    course ||= ::Course.find_by(name: course_name) if course_name.present?
    
    color_val = nil

    if course && course.respond_to?(:custom_color) && course.custom_color.present?
      color_val = course.custom_color
    elsif semester.present?
      prefs = ::UserPreference.current
      if prefs.semester_colors && prefs.semester_colors[semester].present?
        color_val = prefs.semester_colors[semester]
      end
    end

    hue = nil
    if color_val
      custom = color_val.to_s
      if custom.start_with?('#')
        # Hex color: Convert to HSL
        r = custom[1..2].to_i(16)
        g = custom[3..4].to_i(16)
        b = custom[5..6].to_i(16)
        
        r_f = r / 255.0
        g_f = g / 255.0
        b_f = b / 255.0
        max = [r_f, g_f, b_f].max
        min = [r_f, g_f, b_f].min
        h = (max + min) / 2.0

        if max != min
          d = max - min
          case max
          when r_f then h = (g_f - b_f) / d + (g_f < b_f ? 6 : 0)
          when g_f then h = (b_f - r_f) / d + 2
          when b_f then h = (r_f - g_f) / d + 4
          end
          h /= 6.0
        end
        hue = (h * 360).round
      else
        hue = custom.to_i
      end
    end

    if hue.nil?
      prefix = (course_name || "").to_s[0..6].upcase.strip
      # Stable hash using character values
      hash_val = prefix.chars.map(&:ord).reduce(0, :+)
      hue = (hash_val * 137) % 360 # Spread the colors
    end
    
    # Pastel/Light theme: light background, dark text
    bg = "hsl(#{hue}, 85%, 96%)"
    border = "hsl(#{hue}, 40%, 85%)"
    text = "hsl(#{hue}, 80%, 25%)"
    
    "background-color: #{bg}; color: #{text}; border: 1px solid #{border};"
  end

  def course_pill_style(course_name, semester = nil, course_id = nil)
    self.class.course_pill_style(course_name, semester, course_id)
  end

end
