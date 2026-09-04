# frozen_string_literal: true

require_relative 'test_helper'
require_relative '../lib/brilliant/sync/base_service'

module Brilliant
  class TextProcessor
    def self.html_to_markdown(html)
      html.to_s
    end
  end
end

require_relative '../lib/brilliant/sync/notification_service'

class NotificationContentRefreshClient
  attr_reader :toc_requests, :syncs

  def initialize(toc_by_course: {}, raise_on_get_toc: nil)
    @toc_by_course = toc_by_course
    @raise_on_get_toc = raise_on_get_toc
    @toc_requests = []
    @syncs = []
  end

  def get_toc(course_id, force_refresh: false)
    @toc_requests << { course_id: course_id, force_refresh: force_refresh }
    raise @raise_on_get_toc if @raise_on_get_toc

    @toc_by_course[course_id]
  end

  def sync_course_content(course_id, toc)
    @syncs << { course_id: course_id, toc: toc }
  end
end

class NotificationContentRefreshTest < Minitest::Test
  CONTENT = {
    external_id: 'content_100_200',
    course_id: '100',
    notification_type: 'Content',
    title: 'Content Updated: Week 1',
    body: 'Updated body'
  }.freeze

  NEWS = CONTENT.merge(
    external_id: 'news_100_1',
    notification_type: 'News'
  ).freeze

  def test_content_updated_refreshes_brightspace_course_content_with_existing_flow
    toc = { 'Modules' => [{ 'ModuleId' => 1, 'Topics' => [] }] }
    client = NotificationContentRefreshClient.new(toc_by_course: { '100' => toc })
    service = Brilliant::Sync::NotificationService.new(client)

    service.refresh_updated_content_in_brightspace([CONTENT, NEWS])

    assert_equal [{ course_id: '100', force_refresh: true }], client.toc_requests
    assert_equal [{ course_id: '100', toc: toc }], client.syncs
  end

  def test_content_updates_are_grouped_by_course_to_avoid_duplicate_refreshes
    toc = { 'Modules' => [] }
    client = NotificationContentRefreshClient.new(toc_by_course: { '100' => toc })
    service = Brilliant::Sync::NotificationService.new(client)

    service.refresh_updated_content_in_brightspace([
      CONTENT,
      CONTENT.merge(external_id: 'content_100_201')
    ])

    assert_equal 1, client.toc_requests.length
    assert_equal 1, client.syncs.length
    assert_equal '100', client.syncs.first[:course_id]
  end

  def test_missing_content_identifier_logs_but_still_refreshes_course_toc
    toc = { 'Modules' => [] }
    client = NotificationContentRefreshClient.new(toc_by_course: { '100' => toc })
    service = Brilliant::Sync::NotificationService.new(client)

    out, = capture_io do
      service.refresh_updated_content_in_brightspace([CONTENT.merge(external_id: 'content_100_')])
    end

    assert_includes out, 'missing content identifier'
    assert_equal [{ course_id: '100', force_refresh: true }], client.toc_requests
    assert_equal [{ course_id: '100', toc: toc }], client.syncs
  end

  def test_missing_course_id_is_logged_and_skipped_without_crash
    client = NotificationContentRefreshClient.new(toc_by_course: {})
    service = Brilliant::Sync::NotificationService.new(client)

    out, = capture_io do
      service.refresh_updated_content_in_brightspace([CONTENT.merge(course_id: '', external_id: 'content__200')])
    end

    assert_includes out, 'missing course_id'
    assert_empty client.toc_requests
    assert_empty client.syncs
  end

  def test_refresh_failure_is_swallowed_per_course
    client = NotificationContentRefreshClient.new(
      toc_by_course: {},
      raise_on_get_toc: StandardError.new('boom')
    )
    service = Brilliant::Sync::NotificationService.new(client)

    out, = capture_io do
      service.refresh_updated_content_in_brightspace([CONTENT])
    end

    assert_includes out, 'Brightspace content refresh failed for course 100: boom'
    assert_equal [{ course_id: '100', force_refresh: true }], client.toc_requests
    assert_empty client.syncs
  end
end
