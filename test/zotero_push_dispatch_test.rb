# frozen_string_literal: true

# Verifies the dispatch logic that the notification service uses to fan
# brand-new Content notifications out to Zotero. We don't boot ActiveRecord
# for this test — we exercise the same method body in isolation via a host
# class that mirrors NotificationService's interface contract.

require_relative 'test_helper'

# Mirror of Brilliant::Sync::NotificationService#push_new_content_to_zotero
# kept in lockstep with the production implementation. If the production
# method's signature/behavior changes, update both.
class ZoteroPushHost
  def push_new_content_to_zotero(new_items)
    return if new_items.nil? || new_items.empty?
    return unless defined?(Brilliant::Zotero) && Brilliant::Zotero.enabled?

    new_items.each do |n|
      next unless n[:notification_type].to_s == 'Content'
      begin
        Brilliant::Zotero.push_content(n)
      rescue => e
        warn "[ZoteroPushHost] Zotero push failed for #{n[:external_id]}: #{e.message}"
      end
    end
  end

  # Mirror of Brilliant::Sync::NotificationService#push_updated_content_to_zotero.
  def push_updated_content_to_zotero(updated_items)
    return if updated_items.nil? || updated_items.empty?
    return unless defined?(Brilliant::Zotero) && Brilliant::Zotero.enabled?

    updated_items.each do |n|
      next unless n[:notification_type].to_s == 'Content'
      begin
        Brilliant::Zotero.update_content(n)
      rescue => e
        warn "[ZoteroPushHost] Zotero update failed for #{n[:external_id]}: #{e.message}"
      end
    end
  end
end

class ZoteroPushDispatchTest < Minitest::Test
  include ZoteroTestEnv

  CONTENT = {
    external_id: 'content_1', course_id: '1',
    notification_type: 'Content', title: 'Content Updated: A',
    body: 'b', url: '/u', course_name: 'C', date: '2026-01-01T00:00:00Z'
  }.freeze

  NEWS = {
    external_id: 'news_1', course_id: '1',
    notification_type: 'News', title: 'Hi',
    body: 'b', url: '/u', course_name: 'C', date: '2026-01-01T00:00:00Z'
  }.freeze

  def setup
    reset_zotero_client!
    @host = ZoteroPushHost.new
  end

  def teardown
    reset_zotero_client!
  end

  def test_disabled_no_push_attempted
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => nil,
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_new_content_to_zotero([CONTENT, NEWS])
    end
    assert_empty fake.calls
  end

  def test_enabled_only_pushes_content
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_new_content_to_zotero([CONTENT, NEWS])
    end
    assert_equal 1, fake.calls.length, 'News rows must not be pushed'
    assert_equal 'Content Updated: A', fake.calls.first[:title]
  end

  def test_only_called_with_newly_created
    # The service's contract is that this helper only receives items the
    # batch JUST created. Simulate a second sync pass with no new rows.
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_new_content_to_zotero([])
      @host.push_new_content_to_zotero(nil)
    end
    assert_empty fake.calls, 'no new rows -> no Zotero traffic'
  end

  def test_per_item_failure_does_not_abort_batch
    failing_first = FakeZoteroClient.new(raise_on_call: Brilliant::Zotero::Error.new('nope'))
    Brilliant::Zotero.client = failing_first
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      # Two Content items in one batch; the failing client raises for each
      # but the dispatcher must keep going so notification sync isn't broken.
      @host.push_new_content_to_zotero([CONTENT, CONTENT.merge(external_id: 'content_2')])
    end
    assert_equal 2, failing_first.calls.length
  end
end

class ZoteroUpdateDispatchTest < Minitest::Test
  include ZoteroTestEnv

  CONTENT = {
    external_id: 'content_1', course_id: '1',
    notification_type: 'Content', title: 'Content Updated: A',
    body: 'b2', url: '/u', course_name: 'C', date: '2026-01-02T00:00:00Z'
  }.freeze

  NEWS = {
    external_id: 'news_1', course_id: '1',
    notification_type: 'News', title: 'Hi',
    body: 'b', url: '/u', course_name: 'C', date: '2026-01-01T00:00:00Z'
  }.freeze

  def setup
    reset_zotero_client!
    @host = ZoteroPushHost.new
  end

  def teardown
    reset_zotero_client!
  end

  def test_disabled_no_update_attempted
    fake = FakeZoteroClient.new(seed: { 'content_1' => { key: 'K', version: 1, data: {} } })
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => nil,
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_updated_content_to_zotero([CONTENT, NEWS])
    end
    assert_empty fake.finds
    assert_empty fake.updates
  end

  def test_only_content_rows_are_updated
    fake = FakeZoteroClient.new(seed: { 'content_1' => { key: 'K', version: 3, data: {} } })
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_updated_content_to_zotero([CONTENT, NEWS])
    end
    assert_equal ['content_1'], fake.finds, 'News rows must not be resolved/updated'
    assert_equal 1, fake.updates.length
    assert_equal 'K', fake.updates.first[:key]
  end

  def test_missing_identifier_is_handled_gracefully
    fake = FakeZoteroClient.new(seed: {})
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_updated_content_to_zotero([CONTENT.merge(external_id: '')])
    end
    assert_empty fake.finds, 'no identifier -> no resolve attempt'
    assert_empty fake.updates
    assert_empty fake.calls
  end

  def test_per_item_failure_does_not_abort_batch
    failing = FakeZoteroClient.new(raise_on_call: Brilliant::Zotero::Error.new('nope'))
    Brilliant::Zotero.client = failing
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      @host.push_updated_content_to_zotero([CONTENT, CONTENT.merge(external_id: 'content_2')])
    end
    assert_equal 2, failing.finds.length, 'dispatcher keeps going after a failure'
  end
end
