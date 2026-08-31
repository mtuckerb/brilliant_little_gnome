require_relative '../base_controller'

class McpController < BaseController
  # MCP SSE setup
  set :mcp_connections, {}

  before '/api/v1/mcp/*' do
    validate_api_access!
  end

  # Live updates via Model Context Protocol (SSE)
  get '/api/v1/mcp/sse' do
    content_type 'text/event-stream'
    headers 'Cache-Control' => 'no-cache', 'Connection' => 'keep-alive', 'X-Accel-Buffering' => 'no'

    stream(:keep_open) do |out|
      session_id = SecureRandom.uuid
      settings.mcp_connections[session_id] = out

      # Metadata for the client to know where to POST messages
      # The MCP SSE spec requires the endpoint to be sent as the data of the 'endpoint' event.
      # Many clients expect this to be JSON-encoded if they use a generic stream parser.
      out << "event: endpoint\n"
      endpoint_url = "/api/v1/mcp/messages?session_id=#{session_id}"
      endpoint_url += "&api_key=#{params[:api_key]}" if params[:api_key]
      out << "data: #{endpoint_url.to_json}\n\n"

      out.callback do
        settings.mcp_connections.delete(session_id)
      end
    end
  end

  post '/api/v1/mcp/messages' do
    session_id = params[:session_id]
    out = settings.mcp_connections[session_id]

    request_payload = JSON.parse(request.body.read) rescue nil
    halt 400, { error: "Invalid JSON-RPC request" }.to_json unless request_payload

    response_payload = handle_mcp_request(request_payload)

    if response_payload && out
      out << "event: message\n"
      out << "data: #{response_payload.to_json}\n\n"
      status 202
    elsif response_payload
      content_type :json
      response_payload.to_json
    else
      status 204
    end
  end

  helpers do
    def handle_mcp_request(json)
      id = json['id']
      method = json['method']
      params = json['params'] || {}

      case method
      when 'initialize'
        { jsonrpc: "2.0", id: id, result: { protocolVersion: "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "Brilliant-MCP", version: "1.4.3" } } }
      when 'tools/list'
        { jsonrpc: "2.0", id: id, result: { tools: [
          {
            name: "list_courses",
            description: "List enrolled courses with advanced filtering",
            inputSchema: {
              type: "object",
              properties: {
                year: { type: "string", description: "Filter by year (e.g. 2026)" },
                season: { type: "string", description: "Filter by season (e.g. Spring, Fall, Summer)" },
                department_prefix: { type: "string", description: "Filter by department code (e.g. SWO, ECO)" },
                title: { type: "string", description: "Fuzzy match on course title" },
                is_pinned: { type: "boolean", description: "Filter by pinned/favorite status" },
                semester: { type: "string", description: "Exact match for semester string (e.g. '2026 Spring')" }
              }
            }
          },
          {
            name: "get_course_grades",
            description: "Get grade entries for a specific course",
            inputSchema: {
              type: "object",
              properties: {
                course_id: { type: "string", description: "Course OrgUnitId or Course Code (e.g. 'SWO-390' or 'SWO 390')" }
              },
              required: ["course_id"]
            }
          },
          {
            name: "get_notifications",
            description: "Get notifications across all courses with UI-consistent filtering",
            inputSchema: {
              type: "object",
              properties: {
                course_id: { type: "string", description: "Filter by Course OrgUnitId or Course Code" },
                semester: { type: "string", description: "Filter by semester (e.g. '2026 Spring')" },
                urgency: { type: "integer", description: "Filter by urgency level (1-5)" },
                is_personal: { type: "boolean", description: "Filter for personal/direct notifications" },
                show_read: { type: "boolean", description: "Set to true to include read notifications (default: false)" },
                limit: { type: "integer", default: 10, description: "Maximum number of notifications to return" }
              }
            }
          },
          { name: "get_course_assignments", description: "Get assignments and due dates for a course", inputSchema: { type: "object", properties: { course_id: { type: "string", description: "Course OrgUnitId or Code" } }, required: ["course_id"] } },
          { name: "list_synthetic_tasks", description: "List all custom synthetic tasks", inputSchema: { type: "object", properties: { course_id: { type: "string", description: "Course OrgUnitId or Code" } } } },
          {
            name: "get_course_content",
            description: "Get the table of contents for a course: modules (typically organized by week) and their content items (readings, slides, case studies, media). Use this to find out what's assigned for a given week.",
            inputSchema: {
              type: "object",
              properties: {
                course_id: { type: "string", description: "Course OrgUnitId or Code (e.g. 'SWO-393')" },
                week: { type: "integer", description: "Filter to a specific week number. Matches modules whose title contains 'Week N'. Omit to get all modules." },
                include_items: { type: "boolean", description: "Include content items within each module. Default: true" }
              },
              required: ["course_id"]
            }
          }
        ] } }
      when 'tools/call'
        result = call_mcp_tool(params['name'], params['arguments'] || {})
        { jsonrpc: "2.0", id: id, result: result }
      when 'notifications/initialized'
        nil
      else
        { jsonrpc: "2.0", id: id, error: { code: -32601, message: "Method not found: #{method}" } }
      end
    end

    def resolve_course_id(id_or_code)
      return nil if id_or_code.blank?

      # 1. Try as exact org_unit_id (numeric)
      return id_or_code.to_s if id_or_code.to_s.match?(/^\d+$/)

      # 2. Try exact match on code
      course = Course.find_by(code: id_or_code)
      return course.org_unit_id if course

      # 3. Try fuzzy match on name/code
      # Normalize "SWO-390" to "SWO 390"
      normalized = id_or_code.to_s.gsub(/[-_]/, ' ')

      # Try matching the beginning of the name (most common for "SWO 390")
      course = Course.where("name LIKE ?", "#{normalized}%").first
      return course.org_unit_id if course

      # Try matching anywhere in the name
      course = Course.where("name LIKE ?", "%#{normalized}%").first
      return course.org_unit_id if course

      # Try matching the code with hyphen/space normalization if needed
      # (though codes often look like internal IDs)

      id_or_code # fallback to original if nothing found
    end

    def call_mcp_tool(name, args)
      # Resolve course_id if present
      if args['course_id']
        args['course_id'] = resolve_course_id(args['course_id'])
      end

      case name
      when 'list_courses'
        query = Course.all
        query = query.where("semester LIKE ?", "%#{args['year']}%") if args['year']
        query = query.where("semester LIKE ?", "%#{args['season']}%") if args['season']
        query = query.where(semester: args['semester']) if args['semester']
        query = query.where("name LIKE ?", "%#{args['department_prefix']} %") if args['department_prefix']
        query = query.where("name LIKE ?", "%#{args['title']}%") if args['title']
        query = query.where(is_pinned: (args['is_pinned'] == true)) if args.has_key?('is_pinned')

        courses = query.order(is_pinned: :desc, last_accessed_at: :desc)
        { content: [{ type: "text", text: courses.to_json }] }

      when 'get_course_grades'
        grades = Grade.where(course_id: args['course_id']).order(Arel.sql("due_date ASC NULLS LAST"))
        { content: [{ type: "text", text: grades.to_json }] }

      when 'get_notifications'
        query = Notification.all
        query = query.where(course_id: args['course_id']) if args['course_id']
        query = query.where(semester: args['semester']) if args['semester']
        query = query.where(urgency: args['urgency']) if args['urgency']
        query = query.where(is_personal: true) if args['is_personal']
        query = query.where(is_read: false) if args['show_read'] != true

        limit = args['limit'] || 10
        items = query.order(date: :desc).limit(limit)
        { content: [{ type: "text", text: items.to_json }] }

      when 'get_course_assignments'
        assignments = Assignment.where(course_id: args['course_id']).order(Arel.sql('due_date ASC NULLS LAST'))
        { content: [{ type: "text", text: assignments.to_json }] }

      when 'list_synthetic_tasks'
        query = Assignment.where(assignment_type: 'synthetic')
        query = query.where(course_id: args['course_id']) if args['course_id']
        { content: [{ type: "text", text: query.to_json }] }

      when 'get_course_content'
        cid = args['course_id']
        include_items = args.fetch('include_items', true)

        modules = ContentModule.where(course_id: cid).order(:sort_order)

        if args['week']
          week_num = args['week'].to_i
          modules = modules.where("title LIKE ? OR title LIKE ?",
                                  "%Week #{week_num}%",
                                  "%Week_#{week_num}%")
        end

        result = modules.map do |mod|
          entry = {
            id: mod.brightspace_id,
            title: mod.title,
            description: mod.description,
            sort_order: mod.sort_order,
            parent_id: mod.parent_id
          }

          if include_items
            items = ContentItem.where(module_id: mod.brightspace_id)
                               .where(is_hidden: false)
                               .order(:sort_order)
            entry[:items] = items.map do |item|
              {
                id: item.brightspace_id,
                title: item.title,
                type: item.item_type,
                url: item.url,
                attachments: (JSON.parse(item.attachments) rescue item.attachments)
              }
            end
          end

          entry
        end

        { content: [{ type: "text", text: result.to_json }] }

      else
        { isError: true, content: [{ type: "text", text: "Tool not found: #{name}" }] }
      end
    end
  end
end
