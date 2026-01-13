module CourseHelpers
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
  def collect_all_files(module_obj, files = [])
    return files unless module_obj

    # Add topics that are files
    if module_obj['Topics']
      module_obj['Topics'].each do |t|
        if t['Url'] && t['Url'].start_with?('/content/enforced/')
          files << { 
            id: (t['Id'] || t['TopicId']), 
            title: t['Title'], 
            path: "/d2l/api/le/1.40/#{@course_id}/content/topics/#{t['Id'] || t['TopicId']}/file"
          }
        end
      end
    end

    # Recurse into sub-modules
    if module_obj['Modules']
      module_obj['Modules'].each do |sub|
        collect_all_files(sub, files)
      end
    end

    files
  end

  # Recursively collects all files from the entire TOC
  def collect_course_files(toc)
    return [] unless toc && toc['Modules']
    files = []
    toc['Modules'].each do |m|
      collect_all_files(m, files)
    end
    files
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
end
