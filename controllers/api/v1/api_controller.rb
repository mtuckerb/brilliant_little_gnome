require_relative '../../base_controller'

module Api
  module V1
    class ApiController < BaseController
      before '/api/v1/*' do
        @user_prefs = UserPreference.current
        # Skip validation for the re-auth cookie endpoint
        next if request.path_info == '/api/v1/auth/cookies'

        validate_api_access!
        content_type :json unless request.path_info == '/api/v1/events'
      end

      # CORS preflight for browser extension auth (includes Private Network Access)
      options '/api/v1/auth/cookies' do
        headers 'Access-Control-Allow-Origin' => '*',
                'Access-Control-Allow-Methods' => 'POST, OPTIONS',
                'Access-Control-Allow-Headers' => 'Content-Type',
                'Access-Control-Allow-Private-Network' => 'true'
        204
      end

      # Re-authentication from Electron or browser extension
      post '/api/v1/auth/cookies' do
        headers 'Access-Control-Allow-Origin' => '*',
                'Access-Control-Allow-Private-Network' => 'true'
        content_type :json
        begin
          data = JSON.parse(request.body.read)
          host = data['host']
          cookies = data['cookies']

          if host.present? && cookies.present?
            $client.save_connection_config(host.strip, cookies.strip)
            { status: 'ok' }.to_json
          else
            halt 400, { error: "Missing parameters" }.to_json
          end
        rescue => e
          halt 400, { error: "Invalid JSON: #{e.message}" }.to_json
        end
      end

      # Course Discovery
      get '/api/v1/courses' do
        $client.sync_all_courses_proactively
        Course.all.order(is_pinned: :desc, sort_order: :asc, last_accessed_at: :desc).map(&:as_json).to_json
      end

      post '/api/v1/assignments' do
        data = JSON.parse(request.body.read) rescue {}
        
        if data['course_id'].blank? || data['name'].blank?
          halt 400, { error: 'course_id and name are required' }.to_json
        end

        brightspace_id = "syn_#{SecureRandom.hex(8)}"
        
        parsed_date = nil
        if data['due_date'].present?
          Time.use_zone(@user_prefs.time_zone || "UTC") do
            if data['due_time'].present?
              parsed_date = Time.zone.parse("#{data['due_date']} #{data['due_time']}")
            else
              parsed_date = Time.zone.parse(data['due_date']).end_of_day
            end
          end
        end

        assignment = Assignment.create!(
          course_id: data['course_id'],
          brightspace_id: brightspace_id,
          name: data['name'],
          description: data['description'],
          due_date: parsed_date,
          external_url: data['external_url'],
          synthetic: true,
          manually_edited: true,
          manually_edited_at: Time.current
        )

        { status: 'ok', id: assignment.id, brightspace_id: brightspace_id }.to_json
      end

      get '/api/v1/courses/:id/summary' do
        course = Course.find_by(org_unit_id: params[:id])
        halt 404, { error: "Course not found" }.to_json unless course

        info = extract_course_info(course.display_name || "", course&.org_unit_id)
        toc = build_toc_tree(params[:id])

        # Proactively background sync the most important parts of the course
        # This ensures that even if the periodic sync hasn't run, visiting the course
        # refreshes the data.
        Thread.new do
          ActiveRecord::Base.connection_pool.with_connection do
            begin
              course_id = params[:id]
              
              # Sync TOC
              latest_toc = $client.get_toc(course_id, force_refresh: true)
              $client.sync_course_content(course_id, latest_toc) if latest_toc
              
              # Sync Grades
              grades_raw = $client.get_grades(course_id, force_refresh: true)
              $client.sync_grades(course_id, grades_raw) if grades_raw
              
              # Sync Assignments
              assignments = $client.get_assignments(course_id, force_refresh: true)
              $client.sync_assignments(course_id, assignments) if assignments
              
              # Publish update events
              Brilliant::EventBus.publish(:course_overview_updated, { course_id: course_id })
              Brilliant::EventBus.publish(:grades_updated, { course_id: course_id })
              Brilliant::EventBus.publish(:assignments_updated, { course_id: course_id })
            rescue => e
              puts "[API] Background proactive sync failed for #{params[:id]}: #{e.message}"
            end
          end
        end

        upcoming = Assignment.where(course_id: params[:id], completed: false)
                             .where("due_date > ? AND due_date <= ?", Time.current, Time.current + 7.days)
                             .order(due_date: :asc).map do |a|
                               a.as_json.merge({
                                 name_html: render_markdown_inline(a.name),
                                 description_html: render_content(a.description)
                               })
                             end

        overview = nil
        if course.overview_raw.present?
          overview = JSON.parse(course.overview_raw) rescue nil
        end

        # Proactively background sync the overview
        Thread.new do
          ActiveRecord::Base.connection_pool.with_connection do
            begin
              latest_overview = $client.get_overview(params[:id])
              if latest_overview && latest_overview.to_json != course.overview_raw
                course.update_columns(overview_raw: latest_overview.to_json)
                Brilliant::EventBus.publish(:course_overview_updated, { course_id: params[:id] })
              end
            rescue => e
              puts "[API] Background overview sync failed: #{e.message}"
            end
          end
        end

        if overview
          overview['description_html'] = render_content(overview['Description'])
        else
           # Fallback to local-first fetch if still missing
           overview = $client.get_overview(params[:id])
           if overview
             overview['description_html'] = render_content(overview['Description'])
           end
        end

        {
          course: course.as_json.merge(info),
          overview: overview,
          syllabus_info: find_syllabus_items(toc['Modules']),
          upcoming_assignments: upcoming,
          grade_stats: calculate_grade_stats(params[:id]),
          toc: toc
        }.to_json
      end

      get '/api/v1/courses/:id' do
        course = Course.find_by(org_unit_id: params[:id])
        halt 404, { error: "Course not found" }.to_json unless course
        course.to_json
      end

      get '/api/v1/courses/:id/assignments/summary' do
        course_id = params[:id]
        show_completed = params[:show_completed] == 'true'

        # Proactively trigger background sync for assignments and quizzes
        if Assignment.where(course_id: course_id).empty?
          Thread.new do
            ActiveRecord::Base.connection_pool.with_connection do
              assignments = $client.get_assignments(course_id)
              $client.sync_assignments(course_id, assignments) if assignments
              quizzes = $client.get_quizzes(course_id)
              $client.sync_quizzes(course_id, quizzes) if quizzes
              Brilliant::EventBus.publish(:assignments_updated, { course_id: course_id })
            rescue => sync_err
              puts "[API] Background assignment sync failed: #{sync_err.message}"
            end
          end
        end

        query = Assignment.where(course_id: course_id)
        query = query.where(completed: false) unless show_completed

        # NULLS LAST — SQLite's plain ASC would float no-due-date rows to the top.
        assignments = query.order(Arel.sql('due_date ASC NULLS LAST')).map do |a|
          a.as_json.merge({
            name_html: render_markdown_inline(a.name),
            is_completed: a.completed,
            is_optional: a.optional,
            is_quiz: a.assignment_type == 'quiz'
          })
        end

        { assignments: assignments }.to_json
      end

      get '/api/v1/courses/:id/discussions' do
        course_id = params[:id]

        # Proactively trigger a background sync of discussion topics if none exist
        # This makes subsequent loads much faster and ensures metadata (counts) is current
        if DiscussionTopic.where(course_id: course_id).empty?
          Thread.new do
            ActiveRecord::Base.connection_pool.with_connection do
              forums = $client.get_discussions(course_id)
              $client.sync_discussions(course_id, forums) if forums
            rescue => sync_err
              puts "[API] Background discussion sync failed: #{sync_err.message}"
            end
          end
        end

        topics = DiscussionTopic.where(course_id: course_id).order(sort_order: :asc).map do |t|
          t.as_json.merge({
            display_thread_count: t.thread_count,
            display_post_count: t.post_count
          })
        end
        { topics: topics }.to_json
      end

      get '/api/v1/courses/:id/discussions/:forum_id/topics/:topic_id' do
        course_id = params[:id]
        forum_id = params[:forum_id]
        topic_id = params[:topic_id]
        force_refresh = params[:force_refresh] == 'true'

        puts "[API] Loading Topic (Local-First): #{course_id} / #{forum_id} / #{topic_id}"

        begin
          forum = DiscussionForum.find_by(brightspace_id: forum_id, course_id: course_id) || $client.get_discussion_forum(course_id, forum_id)
          topic = DiscussionTopic.find_by(brightspace_id: topic_id, forum_id: forum_id) || $client.get_discussion_topic(course_id, forum_id, topic_id)
          evaluation = $client.get_discussion_evaluation(course_id, forum_id, topic_id)

          # Load posts from DB
          # Participation and threading requires correct user IDs
          current_bs_user_id = @user_prefs.brightspace_user_id.to_s
          
          posts_from_db = DiscussionPost.where(topic_id: topic_id.to_s).order(posted_at: :asc)
          all_posts = posts_from_db.map(&:to_api_hash)

          # Proactively sync in background
          Thread.new do
            ActiveRecord::Base.connection_pool.with_connection do
              Time.zone = UserPreference.current&.time_zone || "UTC"
              posts_data_raw = $client.get_topic_posts(course_id, forum_id, topic_id, force_refresh: force_refresh)
              new_posts = $client.ensure_array(posts_data_raw)
              
              # Capture current user ID from prefs for the sync context
              current_user_bs_id = UserPreference.current&.brightspace_user_id
              DiscussionPost.sync_from_api(course_id, forum_id, topic_id, new_posts)

              # Signal completion if something changed or we were empty
              if posts_from_db.empty? || force_refresh || new_posts.size != posts_from_db.size
                Brilliant::EventBus.publish(:discussion_topic_updated, {
                  course_id: course_id,
                  forum_id: forum_id,
                  topic_id: topic_id,
                  post_count: new_posts.size
                })
              end
            rescue => sync_err
               puts "[API ERROR] Background sync failed: #{sync_err.message}"
            end
          end

          current_bs_user_id = @user_prefs.brightspace_user_id

          # Robust participated check
          participated = false
          if current_bs_user_id.present?
            # Use to_s to ensure string comparison for IDs which can be large integers or strings in API
            participated = DiscussionPost.where(topic_id: topic_id.to_s).where(author_id: current_bs_user_id.to_s).exists?
          end

          threads_groups = all_posts.reject { |p| p.nil? || !p.is_a?(Hash) }.group_by { |p| p['ThreadId'] }
          threads_with_posts = threads_groups.map do |thread_id, posts|
            root_post = posts.find { |p| p['ParentPostId'].nil? || p['ParentPostId'].to_s == "0" || p['ParentPostId'].to_s == "" } || posts.min_by { |p| p['DatePosted'] || "9999" }

            root_author_id = root_post.dig('Author', 'Identifier') || root_post['UserId'] || root_post['PosterId']
            user_is_author = current_bs_user_id.present? && root_author_id.to_s == current_bs_user_id.to_s
            
            # IMPROVED: Flag participation if any post in the thread is from the user
            user_participated = posts.any? { |p| (p.dig('Author', 'Identifier') || p['UserId'] || p['PosterId']).to_s == current_bs_user_id.to_s }

            thread = {
              'ThreadId' => thread_id,
              'Subject' => root_post['Subject'] || "No Subject",
              'PostingUserDisplayName' => root_post['PostingUserDisplayName'],
              'ThreadDate' => root_post['DatePosted'],
              'LastReplyDate' => posts.map { |p| p['DatePosted'] }.compact.max,
              'IsPinned' => root_post['IsPinned'] || false,
              'ReplyCount' => posts.size - 1,
              'UserIsAuthor' => user_is_author,
              'UserParticipated' => user_participated
            }
            { thread: thread, post_tree: build_post_tree(posts) }
          end.sort_by { |item| item[:thread]['IsPinned'] ? 0 : 1 }

          {
            forum: forum,
            topic: topic,
            evaluation: evaluation,
            participated: participated,
            threads_with_posts: threads_with_posts,
            instructions_collapsed: @user_prefs.topic_collapsed?("#{topic_id}:instructions") || participated,
            feedback_collapsed: @user_prefs.topic_collapsed?("#{topic_id}:feedback"),
            from_cache: posts_from_db.any? && !force_refresh
          }.to_json
        rescue => e
          puts "[API ERROR] Discussion Route Crash: #{e.message}"
          puts e.backtrace.first(15).join("\n")
          halt 500, { error: e.message }.to_json
        end
      end

      get '/api/v1/courses/:id/discussions/:forum_id/topics' do
        course_id = params[:id]
        forum_id = params[:forum_id]
        force_refresh = params[:force_refresh] == 'true'

        forum = DiscussionForum.find_by(brightspace_id: forum_id, course_id: course_id) || $client.get_discussion_forum(course_id, forum_id)
        topics = DiscussionTopic.where(forum_id: forum_id, course_id: course_id).order(sort_order: :asc)

        if topics.empty? || force_refresh
          topics_raw = $client.get_discussion_topics(course_id, forum_id, force_refresh: force_refresh)
          topics_data = topics_raw.is_a?(Hash) ? (topics_raw['Items'] || []) : (topics_raw || [])
          Thread.new do
            ActiveRecord::Base.connection_pool.with_connection do
              $client.sync_discussion_topics(course_id, forum_id, topics_data)
            end
          end
          topics = topics_data if topics.empty?
        end

        { forum: forum, topics: topics }.to_json
      end

      get '/api/v1/courses/:id/announcements' do
        course_id = params[:id]

        # Always trigger a targeted background sync for this course
        Thread.new do
          ActiveRecord::Base.connection_pool.with_connection do
            begin
              # Sync just this course's specific announcements
              enrollments = $client.get_enrollments
              course_enrollment = enrollments.select { |e| e['OrgUnit']['Id'].to_s == course_id.to_s }

              if course_enrollment.any?
                $client.sync_notifications(course_enrollment, $client.get_who_am_i, limit: 1)
                Brilliant::EventBus.publish(:notification_received, { course_id: course_id, type: 'News' })
              end
            rescue => e
              puts "[API] Background course news sync failed: #{e.message}"
            end
          end
        end

        # Include both News and Content updates in the announcements feed
        announcements = Notification.where(course_id: course_id, notification_type: ['News', 'Content']).order(date: :desc).limit(50).map do |n|
          n.as_json.merge({
            formatted_date: format_date(n.date, "%b %d"),
            full_date: format_date(n.date),
            body_html: render_content(n.body)
          })
        end

        { announcements: announcements }.to_json
      end

      get '/api/v1/courses/:id/modules/:module_id' do
        course_id = params[:id]
        module_id = params[:module_id]

        course = Course.find_by(org_unit_id: course_id)
        halt 404, { error: "Course not found" }.to_json unless course

        # For the real module data, we still use the client but it's now wrapped in an API call
        # This keeps the main Sinatra thread free for the shell view.
        toc_full = build_toc_tree(course_id)
        module_obj = find_module(toc_full['Modules'], module_id)

        # If not in cache/local tree, fetch from API
        unless module_obj
          module_obj = $client.get_toc(course_id).then { |t| find_module(t['Modules'], module_id) }
        end
        halt 404, { error: "Module not found" }.to_json unless module_obj

        # Sync tasks
        tasks = synthesize_tasks(module_obj, course.display_name)
        tasks.each { |t| t[:name_html] = render_markdown_inline(t[:name]) }

        # Enrich module topics with description_html
        module_obj['description_html'] = render_content(module_obj['Description'])

        {
          course: course,
          module: module_obj,
          synthetic_tasks: tasks,
          course_info: extract_course_info(course.display_name, course&.org_unit_id),
          download_count: (module_obj['Topics'] || []).select { |t| t['Url'] && t['Url'].start_with?('/content/enforced/') }.size
        }.to_json
      end

      get '/api/v1/courses/:id/search' do
        course_id = params[:id]
        query = params[:q]

        toc = build_toc_tree(course_id)
        # Fallback to API if tree is empty
        if toc['Modules'].empty?
          toc = $client.get_toc(course_id)
        end

        results = search_toc(toc['Modules'], query)
        { results: results }.to_json
      end

      get '/api/v1/courses/:id/grades/summary' do
        course_id = params[:id]

        # Proactively trigger background sync for grades
        if Grade.where(course_id: course_id).empty?
          Thread.new do
            ActiveRecord::Base.connection_pool.with_connection do
              begin
                grades_raw = $client.get_grades(course_id)
                $client.sync_grades(course_id, grades_raw) if grades_raw
                Brilliant::EventBus.publish(:grades_updated, { course_id: course_id })
              rescue => sync_err
                puts "[API] Background grade sync failed: #{sync_err.message}"
              end
            end
          end
        end

        # Build a lookup of assignment submission status keyed by grade_item_id
        assignment_submission = Assignment.where(course_id: course_id.to_s)
                                          .where.not(grade_item_id: [nil, ''])
                                          .pluck(:grade_item_id, :completed, :completed_at)
                                          .each_with_object({}) { |(gid, comp, at), h| h[gid.to_s] = { completed: comp, completed_at: at } }

        grades_scope = Grade.where(course_id: course_id)
        grades_scope = grades_scope.where(hidden: false) unless params[:show_hidden] == 'true'
        grades = grades_scope.order(Arel.sql("due_date ASC NULLS LAST"), name: :asc).map do |g|
          numerator = g.effective_numerator
          denominator = g.effective_denominator
          perc = (numerator && denominator && denominator > 0) ? ((numerator / denominator.to_f) * 100).round(1) : nil
          rel_weight = g.weight || 0

          submission = assignment_submission[g.brightspace_id.to_s]
          # submitted is true if a linked assignment is marked completed, false if linked but not completed,
          # nil if there's no linked assignment (e.g. quiz, category total) so the UI can omit the indicator.
          submitted = submission ? submission[:completed] : nil
          # A graded item must have been submitted, regardless of what assignment.completed says
          # (the completed flag from Brightspace is sometimes stale or never set for in-class work).
          submitted = true if !perc.nil?

          g.as_json.merge({
            name_html: render_markdown_inline(g.name),
            numerator: numerator,
            denominator: denominator,
            perc: perc,
            completed: !perc.nil?,
            submitted: submitted,
            submitted_at: submission && submission[:completed_at],
            rel_weight: rel_weight,
            manually_marked_ungraded: g.manually_marked_ungraded,
            hidden: g.hidden,
            expected_score: g.expected_score,
            is_expected: g.is_expected?
          })
        end

        {
          grades: grades,
          grade_stats: calculate_grade_stats(course_id)
        }.to_json
      end

      # Global Notifications Feed
      get '/api/v1/notifications' do
        # Only trigger background sync if explicitly requested or if we have no notifications at all
        if params[:sync] == 'true' || Notification.count == 0
          # Perform the fast Unified Feed sync synchronously to ensure immediate updates are seen
          begin
            courses = $client.get_enrollments
            user = $client.get_who_am_i
            # Get the feed stuff now
            feed_items = $client.get_unified_feed(courses, force_refresh: true)
            $client.upsert_notification_batch(feed_items, publish_event: true)
            
            # Kick off the deep dive in the background
            Thread.new do
              ActiveRecord::Base.connection_pool.with_connection do
                begin
                  $client.sync_notifications(courses, user)
                rescue => e
                  puts "[API] Background deep notification sync failed: #{e.message}"
                end
              end
            end
          rescue => e
            puts "[API] Synchronous notification sync failed: #{e.message}"
          end
        end

        # Subquery to deduplicate by course_id and title, picking the latest for each
        dedup_query = Notification.select("MAX(id) as id")
        dedup_query = dedup_query.where(course_id: params[:course_id]) if params[:course_id].present?
        dedup_query = dedup_query.where(semester: params[:semester]) if params[:semester].present?
        dedup_query = dedup_query.where(urgency: params[:urgency]) if params[:urgency].present?
        dedup_query = dedup_query.where(notification_type: params[:type]) if params[:type].present?
        dedup_query = dedup_query.where(is_personal: true) if params[:personal_only] == 'true'
        dedup_query = dedup_query.where(is_read: false) unless params[:show_read] == 'true'
        dedup_query = dedup_query.group(:course_id, :title)

        query = Notification.where(id: dedup_query)

        total = query.count
        sort_by = params[:sort] || 'date'
        query = (sort_by == 'urgency') ? query.order(urgency: :desc, date: :desc, id: :desc) : query.order(date: :desc, id: :desc)

        limit = (params[:limit] || 25).to_i
        offset = (params[:offset] || 0).to_i

        # Pre-cache course models to avoid N+1 in the loop below
        course_map = Course.where(org_unit_id: query.limit(limit).offset(offset).pluck(:course_id).uniq).index_by(&:org_unit_id)

        notifications = query.limit(limit).offset(offset).map do |n|
          course = course_map[n.course_id.to_s]
          course_name_to_use = course&.display_name.presence || n.course_name
          if (course_name_to_use.nil? || course_name_to_use.match?(/^\d+$/)) && course
            course_name_to_use = course.display_name
          end

          info = extract_course_info(course_name_to_use || "", course&.org_unit_id)

          # Force the pill_style to use the custom_color from the actual course handle if available
          pill_style = info[:pill_style]
          if course && course.custom_color.present?
            pill_style = CourseHelpers.course_pill_style(course.display_name, info[:semester], course.org_unit_id)
          end

          n.as_json.merge({
            display_title: clean_notification_title(n.title, course_name_to_use),
            course_prefix: info[:prefix],
            course_short_name: info[:short_name] || course_name_to_use,
            formatted_date: format_date(n.date),
            simple_date: format_date(n.date, "%b %d"),
            body_html: render_content(n.body),
            pill_style: pill_style
          })
        end

        { total: total, limit: limit, offset: offset, notifications: notifications }.to_json
      end

      get '/api/v1/dashboard/summary' do
        $client.sync_all_courses_proactively
        summary_data = get_dashboard_summary_data(params[:overview_semester])

        {
          overall_gpa: summary_data[:overall_gpa],
          max_potential_gpa: summary_data[:max_potential_gpa],
          semester: summary_data[:semester],
          all_semesters: summary_data[:all_semesters],
          cumulative_points_earned: summary_data[:cumulative_points_earned],
          cumulative_points_possible: summary_data[:cumulative_points_possible],
          show_upcoming_assignments: @user_prefs.show_upcoming_assignments,
          show_course_list: @user_prefs.show_course_list,
          show_recent_updates: @user_prefs.show_recent_updates,
          semester_grades: summary_data[:semester_grades].map { |sg|
             c = sg[:course]
             info = extract_course_info(c.display_name || "", c.org_unit_id)
             {
               org_unit_id: c.org_unit_id,
               name: c.display_name,
               short_name: info[:short_name],
               prefix: info[:prefix],
               pill_style: info[:pill_style],
               code: info[:course_display] || c.code,
               banner_url: c.banner_url,
               stats: sg[:stats]
             }
          },
          courses: Course.all.order(is_pinned: :desc, sort_order: :asc, last_accessed_at: :desc).limit(100).map { |c|
             info = extract_course_info(c.display_name || "", c.org_unit_id)
             {
               org_unit_id: c.org_unit_id,
               name: c.display_name,
               short_name: info[:short_name],
               prefix: info[:prefix],
               pill_style: info[:pill_style],
               code: info[:course_display] || c.code,
               banner_url: c.banner_url,
               is_pinned: c.is_pinned,
               semester: c.semester
             }
          },
          upcoming_assignments: Assignment.where(completed: false).where("due_date > ? AND due_date <= ?", Time.current, Time.current + 14.days).order(optional: :asc, due_date: :asc).limit(15).map { |a|
            c_name = a.course&.display_name || ""
            info = extract_course_info(c_name, a.course_id)
            a.as_json.merge(
              'course_name' => c_name,
              'course_short_name' => info[:short_name],
              'course_prefix' => info[:prefix],
              'pill_style' => info[:pill_style],
              'name_html' => render_markdown_inline(a.name)
            )
          },
          recent_notifications: Notification.where(is_read: false).order(date: :desc).limit(10).map { |n|
            course = Course.find_by(org_unit_id: n.course_id.to_s)
            c_name = course&.display_name.presence || n.course_name
            if (c_name.to_s.empty? || c_name.to_s.match?(/^\d+$/)) && course
              c_name = course.display_name if course.display_name.present? && !course.display_name.to_s.match?(/^\d+$/)
            end

            info = extract_course_info(c_name || "", n.course_id)

            # Use custom color if available
            pill_style = info[:pill_style]
            if course && course.respond_to?(:custom_color) && course.custom_color.present?
              pill_style = CourseHelpers.course_pill_style(course.display_name, info[:semester], course&.org_unit_id)
            end

            n.as_json.merge(
              'title' => clean_notification_title(n.title, c_name),
              'course_name' => c_name,
              'course_short_name' => info[:short_name] || c_name,
              'course_prefix' => info[:prefix],
              'pill_style' => pill_style
            )
          }
        }.to_json
      end

      get '/api/v1/preferences' do
        @user_prefs.to_json(except: [:api_key, :brightspace_cookie])
      end

      get '/api/v1/token' do
        token = generate_jwt_token({ user_id: 'internal', display_name: @user_prefs.display_name })
        { token: token }.to_json
      end

      get '/api/v1/status' do
        {
          authenticated: $client.authenticated?,
          degraded_mode: $client.degraded_mode,
          host: $client.host,
          sync_status: $client.sync_status,
          oauth_enabled: !!(ENV['BS_CLIENT_ID'] && ENV['BS_CLIENT_SECRET'])
        }.to_json
      end

      get '/api/v1/search' do
        query = params[:q].to_s.strip
        if query.empty?
          { results: [] }.to_json
        else
          { results: global_search(query) }.to_json
        end
      end

      post '/api/v1/courses/reorder' do
        validate_api_access!
        data = JSON.parse(request.body.read)
        ids = data['ids'] # Array of org_unit_ids
        
        if ids && ids.is_a?(Array)
          ids.each_with_index do |org_unit_id, index|
            Course.where(org_unit_id: org_unit_id.to_s).update_all(sort_order: index)
          end
          { status: 'ok' }.to_json
        else
          halt 400, { error: "Missing ids array" }.to_json
        end
      end

      # Live updates via Server-Sent Events
      get '/api/v1/events' do
        content_type 'text/event-stream'
        headers 'Cache-Control' => 'no-cache', 'Connection' => 'keep-alive', 'X-Accel-Buffering' => 'no'

        queue = Queue.new
        Brilliant::EventBus.subscribe(queue)

        stream(:keep_open) do |out|
          puts "[SSE] Client connection established."

          # Immediately send a connection confirmation
          out << ": connected\n\n" rescue nil

          heartbeat_thread = Thread.new do
            begin
              loop do
                sleep 30
                break if out.closed?
                out << ": heartbeat\n\n"
              end
            rescue => e
              # Socket likely closed
              queue << :stop # Wake up main loop to exit
            end
          end

          begin
            loop do
              event_data = queue.pop
              break if event_data == :stop || out.closed?

              out << "event: #{event_data[:event]}\n" rescue break
              out << "data: #{event_data[:data].to_json}\n\n" rescue break
            end
          rescue => e
            puts "[SSE] Main stream loop closed: #{e.message}"
          ensure
            heartbeat_thread.kill if heartbeat_thread
            Brilliant::EventBus.unsubscribe(queue)
            out.close rescue nil
            puts "[SSE] Client connection cleaned up."
          end
        end
      end

      get '/api/v1/calendar' do
        @view = params[:view] || 'week'
        tz_name = @user_prefs&.time_zone || "UTC"
        Time.zone = tz_name

        @date = params[:date] ? Date.parse(params[:date]) : Time.zone.today
        @show_completed = params[:show_completed] == 'true'

        @weeks_before = (params[:weeks_before] || 0).to_i
        @weeks_after = (params[:weeks_after] || 1).to_i

        if @view == 'week'
          @start_date = @date - (@weeks_before * 7).days
          @end_date = @date + (@weeks_after * 7).days + 6.days
        else
          @start_date = @date.beginning_of_week(:sunday)
          @end_date = @start_date + 34.days
        end

        start_time = @start_date.to_time.beginning_of_day
        end_time = @end_date.to_time.end_of_day

        assignments = Assignment.includes(:course).where(due_date: start_time..end_time)
        grades = Grade.includes(:course).where(due_date: start_time..end_time)

        unless @show_completed
          assignments = assignments.where(completed: false)
          grades = grades.where(numerator: nil)
          completed_assignment_names = Assignment.where(completed: true).pluck(:name)
          grades = grades.to_a.reject { |g| completed_assignment_names.include?(g.name) }
        end

        items_by_date = {}
        (@start_date..@end_date).each { |d| items_by_date[d.to_s] = [] }

        assignments.each do |a|
          date_key = a.due_date.in_time_zone(tz_name).to_date.to_s
          items_by_date[date_key] ||= []
          c_name = a.course&.display_name
          info = extract_course_info(a.course&.display_name || "", a.course&.org_unit_id)

          items_by_date[date_key] << {
            type: 'assignment', id: a.brightspace_id, db_id: a.id, course_id: a.course_id,
            course_name: c_name, course_prefix: info[:prefix], course_short_name: info[:short_name],
            pill_style: info[:pill_style], name: a.name, name_html: render_markdown_inline(a.name),
            description: a.description, external_url: a.external_url, synthetic: a.synthetic,
            date: date_key,
            time: a.due_date, time_display: a.due_date.in_time_zone(tz_name).strftime("%I:%M %p"),
            completed: a.completed, optional: a.optional || false, url: "/course/#{a.course_id}/assignments/#{a.brightspace_id}"
          }
        end

        grades.each do |g|
          date_key = g.due_date.in_time_zone(tz_name).to_date.to_s
          items_by_date[date_key] ||= []
          next if items_by_date[date_key].any? { |e| e[:name] == g.name }

          c_name = g.course&.display_name
          info = extract_course_info(g.course&.display_name || "", g.course&.org_unit_id)

          items_by_date[date_key] << {
            type: 'grade', id: g.brightspace_id, db_id: g.id, course_id: g.course_id,
            course_name: c_name, course_prefix: info[:prefix], course_short_name: info[:short_name],
            pill_style: info[:pill_style], name: g.name, name_html: render_markdown_inline(g.name),
            date: date_key,
            time: g.due_date, time_display: g.due_date.in_time_zone(tz_name).strftime("%I:%M %p"),
            completed: !g.effective_numerator.nil?,
            url: "/course/#{g.course_id}/grades"
          }
        end

        items_by_date.each { |date, items| items_by_date[date] = items.sort_by { |i| [i[:optional] ? 1 : 0, i[:time]] } }

        {
          view: @view, start_date: @start_date, end_date: @end_date, current_date: @date,
          show_completed: @show_completed, weeks_before: @weeks_before, weeks_after: @weeks_after,
          items_by_date: items_by_date, today: Time.zone.today.to_s
        }.to_json
      end

      get '/api/v1/item_details' do
        type = params[:type]
        item_id = params[:id]
        course_id = params[:course_id]

        html = ""
        case type
        when 'assignment'
          if item_id.to_s.start_with?('syn_')
            rec = Assignment.find_by(brightspace_id: item_id, course_id: course_id)
            html = "<div class='mb-3'><h6 class='is-size-7 has-text-weight-bold mb-2'><i class='fas fa-info-circle mr-1 has-text-primary'></i> Instructions / Notes</h6><div class='box is-light p-3' style='box-shadow: none; border: 1px solid #efefef;'>#{render_content(rec.description)}</div></div>" if rec
          else
            assignment = $client.get_assignment(course_id, item_id)
            desc_text = assignment ? (assignment['Instructions'] || assignment['CustomInstructions'] || assignment['Description']) : ""
            desc_text = desc_text.is_a?(Hash) ? (desc_text['Html'] || desc_text['Text'] || "") : desc_text.to_s

            if desc_text.strip.empty?
              rec = Assignment.find_by(brightspace_id: item_id, course_id: course_id)
              desc_text = rec.description if rec && rec.description.present?
            end

            html = desc_text.present? ? "<div class='mb-3'><h6 class='is-size-7 has-text-weight-bold mb-2'><i class='fas fa-info-circle mr-1 has-text-primary'></i> Instructions</h6><div class='box is-light p-3' style='box-shadow: none; border: 1px solid #efefef;'>#{render_content(desc_text)}</div></div>" : ""

            # Include feedback from dropbox if available
            begin
              feedback = $client.get_assignment_feedback(course_id, item_id)
              if feedback
                fb_desc = feedback['Feedback']
                fb_text = fb_desc.is_a?(Hash) ? (fb_desc['Html'] || fb_desc['Text'] || "") : fb_desc.to_s
                if fb_text.strip.present?
                  html += "<div class='mb-3'><h6 class='is-size-7 has-text-weight-bold mb-2'><i class='fas fa-comment-dots mr-1 has-text-success'></i> Feedback</h6><div class='box is-light p-3' style='box-shadow: none; border: 1px solid #efefef; border-left: 3px solid #48c774;'>#{render_content(fb_text)}</div></div>"
                end
              end
            rescue => e
              # Feedback not available — that's fine
            end

            # Include grade/score info if available
            local_rec = Assignment.find_by(brightspace_id: item_id, course_id: course_id)
            grade = nil
            grade = Grade.find_by(brightspace_id: local_rec.grade_item_id, course_id: course_id) if local_rec&.grade_item_id
            grade ||= Grade.where(course_id: course_id).where("LOWER(name) = ?", (assignment&.dig('Name') || "").downcase).first if assignment&.dig('Name')
            if grade
              score_html = ""
              if grade.displayed_grade.present?
                score_html += "<span class='tag is-warning is-light'><strong>Score: #{grade.displayed_grade}</strong></span> "
              end
              if grade.comments.present?
                score_html += "<div class='mt-2'>#{render_content(grade.comments)}</div>"
              end
              if score_html.present?
                html += "<div class='mb-3'><h6 class='is-size-7 has-text-weight-bold mb-2'><i class='fas fa-award mr-1 has-text-warning'></i> Grade</h6><div class='box is-light p-3' style='box-shadow: none; border: 1px solid #efefef; border-left: 3px solid #ffdd57;'>#{score_html}</div></div>"
              end
            end

            html = "<p class='has-text-grey italic'>No details available.</p>" if html.strip.empty?
          end
        when 'topic'
          topic = $client.get_toc(course_id).then { |toc| find_topic(toc['Modules'], item_id) }
          if topic
            desc_text = topic['Description']
            desc_text = desc_text.is_a?(Hash) ? (desc_text['Html'] || desc_text['Text'] || "") : desc_text.to_s
            html = desc_text.present? ? "<div class='mb-3'><h6 class='is-size-7 has-text-weight-bold mb-2'><i class='fas fa-info-circle mr-1 has-text-primary'></i> Description / Contents</h6><div class='box is-light p-3' style='box-shadow: none; border: 1px solid #efefef;'>#{render_content(desc_text)}</div></div>" : "<p class='has-text-grey italic'>No description available.</p>"

            if topic['Url'] && topic['Url'].start_with?('/content/enforced/')
               html += "<div class='mt-4'><a href='/course/#{course_id}/download?path=#{URI.encode_www_form_component(topic['Url'])}&name=#{URI.encode_www_form_component(topic['Title'])}' class='button is-primary is-small'><i class='fas fa-download mr-1'></i> Download File</a></div>"
            end
          end
        when 'grade'
          grade = Grade.find_by(brightspace_id: item_id, course_id: course_id)
          html = "<div class='mb-3'><h6 class='is-size-7 has-text-weight-bold mb-2'><i class='fas fa-comment-dots mr-1 has-text-success'></i> Gradebook Comments</h6><div class='box is-light p-3' style='box-shadow: none; border: 1px solid #efefef;'>#{render_content(grade.comments)}</div></div>" if grade && grade.comments.present?
        end
        { html: html }.to_json
      end

      post '/api/v1/assignments' do
        data = JSON.parse(request.body.read) rescue {}
        course_id = data['course_id']
        name = data['name']
        
        halt 400, { status: 'error', error: "Course and Name are required" }.to_json unless course_id && name

        # Generate unique ID
        brightspace_id = "syn_#{SecureRandom.hex(8)}"
        
        parsed_date = nil
        if data['due_date'].present?
          tz_name = @user_prefs&.time_zone || "UTC"
          Time.use_zone(tz_name) do
            parsed_date = data['due_time'].present? ? Time.zone.parse("#{data['due_date']} #{data['due_time']}") : Time.zone.parse(data['due_date']).end_of_day
          end
        end

        begin
          assignment = Assignment.create!(
            course_id: course_id,
            brightspace_id: brightspace_id,
            name: name,
            description: data['description'],
            due_date: parsed_date,
            external_url: data['external_url'],
            synthetic: true,
            manually_edited: true,
            manually_edited_at: Time.current,
            user_id: @user_prefs&.user_id # Ensure ownership
          )
          { status: 'ok', id: assignment.id, brightspace_id: brightspace_id }.to_json
        rescue => e
          status 422
          { status: 'error', error: e.message }.to_json
        end
      end
    end
  end
end
