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


  def find_module(modules, id)
    return nil unless modules
    modules.each do |m|
      return m if m['ModuleId'].to_s == id.to_s
      found = find_module(m['Modules'], id)
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
  def collect_all_files(module_obj, folder_name, files = [])
    return files unless module_obj

    # Add topics that are files
    if module_obj['Topics']
      module_obj['Topics'].each do |t|
        if t['Url'] && t['Url'].start_with?('/content/enforced/')
          files << { 
            id: (t['Id'] || t['TopicId']), 
            title: t['Title'], 
            path: "/d2l/api/le/1.40/#{@course_id}/content/topics/#{t['Id'] || t['TopicId']}/file",
            folder: folder_name
          }
        elsif t['Url'] && t['Type'] == 3 # Link type topic
          safe_title = t['Title'].gsub(/[^0-9a-z]/i, '_')
          files << {
            title: "#{safe_title}.url",
            content: "[InternetShortcut]\r\nURL=#{t['Url']}\r\n",
            folder: folder_name
          }
        end
      end
    end

    # Recurse into sub-modules but keep the top-level folder name for grouping
    if module_obj['Modules']
      module_obj['Modules'].each do |sub|
        collect_all_files(sub, folder_name, files)
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
      files << {
        title: "Syllabus_Overview.pdf",
        path: "/d2l/api/le/1.40/#{course_id}/overview/attachment",
        folder: "Syllabus_Overview"
      }
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
        collect_all_files(m, folder_name, files)
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

  def extract_course_info(full_name)
    # Pattern: SWO 370:0001-Human Behav (Online) (2026 Spring)
    regex = /^(\w{3}\s\d{3}):\d{4}-(.*?)\s*(\(Online\))?\s*(\(\d{4}\s+(?:Spring|Fall|Summer|Winter)\))$/i
    match = full_name.match(regex)
    
    if match
      {
        course_display: "#{match[1]} - #{match[2].strip}",
        is_online: !match[3].to_s.empty?,
        semester: match[4].to_s.gsub(/[()]/, '').strip
      }
    else 
      # Fallback logic
      is_online = full_name.downcase.include?('online')
      semester_match = full_name.match(/(\(?(?:\d{4}\s+(?:Spring|Fall|Summer|Winter))\)?)/i)
      semester = semester_match ? semester_match[1].gsub(/[()]/, '').strip : nil
      
      course_display = full_name
      if semester
        course_display = course_display.gsub(/\(#{Regexp.escape(semester)}\)/, '').gsub(semester, '').strip
      end
      course_display = course_display.gsub('(Online)', '').strip
      
      {
        course_display: course_display,
        is_online: is_online,
        semester: semester
      }
    end
  end

  # NEW: Search TOC for topics or modules matching a query
  def search_toc(modules, query, results = { modules: [], topics: [] })
    return results unless modules && query && !query.empty?
    
    q = query.downcase
    modules.each do |m|
      if m['Title'].downcase.include?(q)
        results[:modules] << { id: m['ModuleId'], title: m['Title'] }
      end
      
      if m['Topics']
        m['Topics'].each do |t|
          if t['Title'].downcase.include?(q)
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
    year = Time.now.year
    
    # Pattern 1: M/D (1/19)
    date_match = module_title.match(/(\d{1,2}\/\d{1,2})/)
    if date_match
      begin
        month, day = date_match[1].split('/').map(&:to_i)
        inferred_date = Time.new(year, month, day, 23, 59, 59)
      rescue; end
    elsif (month_match = module_title.match(/(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\.?\s*,?\s*(\d{1,2})/i))
      # Pattern 2: Month Day (January 26 or Jan. 26 or January, 26)
      begin
        parsed = Time.parse("#{month_match[1]} #{month_match[2]}")
        inferred_date = Time.new(parsed.year, parsed.month, parsed.day, 23, 59, 59)
      rescue; end
    end

    inferred_date ||= parent_date

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
      text = desc_text.gsub(/<br\s*\/?>/i, "\n").gsub(/<\/p>/i, "\n").gsub(/<[^>]+>/, ' ')
      lines = text.split("\n").map(&:strip).reject(&:empty?)
      
      current_category = "Task"
      lines.each do |line|
        if line =~ /^[A-Z\s]{4,}:?$/ || line =~ /^(Readings|Assignments|Tasks|To Do|Review Questions|Activities|Objectives|Watch):?$/i
          current_category = line.gsub(':', '').strip
          next
        end

        next if current_category =~ /Review Questions|Objectives|Resources/i

        content = nil
        if line =~ /^[\-•*]\s*(.*+)/ || line =~ /^\d+\.\s*(.*+)/
          content = $1 || line
        elsif current_category != "Task" && line.split.size >= 2 && line.split.size <= 8
          content = line
        end

        if content && content.length > 2
          next if tasks.any? { |t| t[:name].downcase == content.strip.downcase }
          
          tasks << { 
            name: content.strip, 
            type: current_category,
            due_date: inferred_date,
            source: 'text',
            module_id: module_obj['ModuleId'],
            context: module_title,
            synthetic: true,
            external_url: "/d2l/le/content/#{@course_id}/viewContent/#{module_obj['ModuleId']}/View"
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
end
