module CourseHelpers
  def truncate_text(text, max_length = 20)
    return text if text.nil? || text.length <= max_length
    text[0...max_length-1] + "…"
  end

  def author_name(obj)
    return "Anonymous" unless obj
    
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

  def find_syllabus_module(modules)
    return nil unless modules
    modules.each do |m|
      return m if m['Title'].downcase.include?('syllabus')
      return m if m['Title'].downcase.include?('overview')
      found = find_syllabus_module(m['Modules'])
      return found if found
    end
    nil
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
    files << {
      title: "Syllabus_Overview.pdf",
      path: "/d2l/api/le/1.40/#{course_id}/overview/attachment",
      folder: "Syllabus_Overview"
    }

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
end
