# frozen_string_literal: true

require 'net/http'
require 'uri'
require 'json'

module Brilliant
  # Brilliant -> Zotero glue.
  #
  # Goal (per the project task):
  #   When Zotero integration is enabled, push newly discovered Brightspace
  #   content (notification_type == 'Content') into the user's Zotero library
  #   once, without blocking notification sync if Zotero is unreachable.
  #
  # Activation is purely env-driven so we don't need a UserPreference schema
  # migration just to ship the integration boundary:
  #
  #   ZOTERO_ENABLED=true
  #   ZOTERO_API_KEY=<personal API key>
  #   ZOTERO_USER_ID=<numeric Zotero user id>
  #
  # Tests stub Brilliant::Zotero.client / Brilliant::Zotero::Client#post_item
  # rather than hitting the network.
  module Zotero
    API_BASE = 'https://api.zotero.org'

    class Error < StandardError; end

    def self.enabled?
      truthy?(ENV['ZOTERO_ENABLED']) &&
        !ENV['ZOTERO_API_KEY'].to_s.strip.empty? &&
        !ENV['ZOTERO_USER_ID'].to_s.strip.empty?
    end

    def self.client
      @client ||= Client.new(
        api_key:  ENV['ZOTERO_API_KEY'],
        user_id:  ENV['ZOTERO_USER_ID'],
        api_base: ENV['ZOTERO_API_BASE'] || API_BASE
      )
    end

    # Test/reset hook.
    def self.client=(c)
      @client = c
    end

    # Push one Brightspace content notification into Zotero.
    #
    # `notification_data` is the same Hash shape that
    # NotificationService#upsert_notification_batch normalises into:
    #   external_id, title, body, url, course_name, date, notification_type
    #
    # Returns truthy on success (or when integration is disabled — a no-op
    # is considered a success from the caller's perspective). Never raises;
    # all errors are logged and swallowed so notification sync is never
    # blocked by Zotero.
    def self.push_content(notification_data)
      return false unless enabled?
      return false unless notification_data
      return false unless notification_data[:notification_type].to_s == 'Content'

      payload = build_item_payload(notification_data)
      client.post_item(payload)
      true
    rescue StandardError => e
      warn "[Brilliant::Zotero] push_content failed: #{e.class}: #{e.message}"
      false
    end

    def self.build_item_payload(n)
      title = n[:title].to_s
      title = 'Brightspace Content' if title.empty?

      note_lines = []
      note_lines << "Course: #{n[:course_name]}" if n[:course_name].to_s.length.positive?
      note_lines << ''
      note_lines << n[:body].to_s if n[:body]
      extra_lines = []
      extra_lines << "brilliant_external_id: #{n[:external_id]}" if n[:external_id]
      extra_lines << "brilliant_course_id: #{n[:course_id]}" if n[:course_id]

      {
        itemType: 'webpage',
        title: title,
        url: n[:url].to_s,
        accessDate: (n[:date] ? n[:date].to_s : nil),
        abstractNote: note_lines.join("\n").strip,
        extra: extra_lines.join("\n")
      }.compact
    end

    def self.truthy?(value)
      %w[1 true yes on].include?(value.to_s.strip.downcase)
    end

    class Client
      attr_reader :api_key, :user_id, :api_base

      def initialize(api_key:, user_id:, api_base: API_BASE)
        @api_key = api_key
        @user_id = user_id
        @api_base = api_base
      end

      # POST a single item to the user's Zotero library. Zotero's API accepts
      # an array of item objects at /users/<id>/items.
      def post_item(item)
        uri = URI.parse("#{api_base}/users/#{user_id}/items")
        req = Net::HTTP::Post.new(uri)
        req['Zotero-API-Key'] = api_key
        req['Content-Type'] = 'application/json'
        req.body = JSON.generate([item])

        http = Net::HTTP.new(uri.host, uri.port)
        http.use_ssl = uri.scheme == 'https'
        http.open_timeout = 5
        http.read_timeout = 10

        res = http.request(req)
        unless res.is_a?(Net::HTTPSuccess)
          raise Error, "Zotero API #{res.code}: #{res.body.to_s[0, 200]}"
        end

        res
      end
    end
  end
end
