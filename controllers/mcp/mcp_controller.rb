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
      out << "event: endpoint\n"
      out << "data: /api/v1/mcp/messages?session_id=#{session_id}\n\n"
      
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
                course_id: { type: "string", description: "Course OrgUnitId" } 
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
                course_id: { type: "string", description: "Filter by Course OrgUnitId" },
                semester: { type: "string", description: "Filter by semester (e.g. '2026 Spring')" },
                urgency: { type: "integer", description: "Filter by urgency level (1-5)" },
                is_personal: { type: "boolean", description: "Filter for personal/direct notifications" },
                show_read: { type: "boolean", description: "Set to true to include read notifications (default: false)" },
                limit: { type: "integer", default: 10, description: "Maximum number of notifications to return" }
              } 
            } 
          },
          { name: "get_course_assignments", description: "Get assignments and due dates for a course", inputSchema: { type: "object", properties: { course_id: { type: "string" } }, required: ["course_id"] } },
          { name: "list_synthetic_tasks", description: "List all custom synthetic tasks", inputSchema: { type: "object", properties: { course_id: { type: "string" } } } }
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

    def call_mcp_tool(name, args)
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
        assignments = Assignment.where(course_id: args['course_id']).order(due_date: :asc)
        { content: [{ type: "text", text: assignments.to_json }] }

      when 'list_synthetic_tasks'
        query = Assignment.where(assignment_type: 'synthetic')
        query = query.where(course_id: args['course_id']) if args['course_id']
        { content: [{ type: "text", text: query.to_json }] }

      else
        { isError: true, content: [{ type: "text", text: "Tool not found: #{name}" }] }
      end
    end
  end
end
