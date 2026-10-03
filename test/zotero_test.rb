# frozen_string_literal: true

require_relative 'test_helper'

class ZoteroEnabledTest < Minitest::Test
  include ZoteroTestEnv

  def setup
    reset_zotero_client!
  end

  def teardown
    reset_zotero_client!
  end

  def test_enabled_false_when_flag_missing
    with_env('ZOTERO_ENABLED' => nil,
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.enabled?
    end
  end

  def test_enabled_false_when_api_key_missing
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => '',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.enabled?
    end
  end

  def test_enabled_false_when_user_id_missing
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '') do
      refute Brilliant::Zotero.enabled?
    end
  end

  def test_enabled_true_when_all_set
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      assert Brilliant::Zotero.enabled?
    end
  end
end

class ZoteroPushTest < Minitest::Test
  include ZoteroTestEnv

  CONTENT_NOTIFICATION = {
    external_id: 'content_42_demo',
    course_id: '42',
    notification_type: 'Content',
    title: 'Content Updated: Week 3 slides',
    body: 'New slides uploaded',
    url: '/course/42/content/demo',
    course_name: 'PSY 220',
    date: '2026-01-15T10:00:00Z'
  }.freeze

  def setup
    reset_zotero_client!
  end

  def teardown
    reset_zotero_client!
  end

  def test_push_content_noop_when_disabled
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => nil,
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.push_content(CONTENT_NOTIFICATION)
    end
    assert_empty fake.calls, 'no push should be attempted when Zotero is off'
  end

  def test_push_content_calls_client_when_enabled
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      assert Brilliant::Zotero.push_content(CONTENT_NOTIFICATION)
    end
    assert_equal 1, fake.calls.length
    item = fake.calls.first
    assert_equal 'Content Updated: Week 3 slides', item[:title]
    assert_equal 'webpage', item[:itemType]
    assert_includes item[:extra].to_s, 'brilliant_external_id: content_42_demo'
    assert_includes item[:extra].to_s, 'brilliant_course_id: 42'
  end

  def test_push_content_skips_non_content_types
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.push_content(
        CONTENT_NOTIFICATION.merge(notification_type: 'News')
      )
    end
    assert_empty fake.calls
  end

  def test_push_content_swallows_client_errors
    fake = FakeZoteroClient.new(raise_on_call: Brilliant::Zotero::Error.new('boom'))
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.push_content(CONTENT_NOTIFICATION),
             'a client failure must not raise out of push_content'
    end
    assert_equal 1, fake.calls.length
  end

  def test_push_content_nil_input_is_safe
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.push_content(nil)
    end
    assert_empty fake.calls
  end
end

# Exercises Brilliant::Zotero.update_content, the "Content Updated" refresh
# path that resolves an existing library item by external_id and PATCHes it
# in place instead of creating a duplicate.
class ZoteroUpdateTest < Minitest::Test
  include ZoteroTestEnv

  CONTENT_NOTIFICATION = {
    external_id: 'content_42_demo',
    course_id: '42',
    notification_type: 'Content',
    title: 'Content Updated: Week 3 slides (v2)',
    body: 'Slides revised',
    url: '/course/42/content/demo',
    course_name: 'PSY 220',
    date: '2026-01-20T10:00:00Z'
  }.freeze

  def setup
    reset_zotero_client!
  end

  def teardown
    reset_zotero_client!
  end

  def with_zotero_env(&block)
    with_env('ZOTERO_ENABLED' => 'true',
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123', &block)
  end

  def test_update_content_noop_when_disabled
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_env('ZOTERO_ENABLED' => nil,
             'ZOTERO_API_KEY' => 'k',
             'ZOTERO_USER_ID' => '123') do
      refute Brilliant::Zotero.update_content(CONTENT_NOTIFICATION)
    end
    assert_empty fake.calls
    assert_empty fake.updates
    assert_empty fake.finds
  end

  def test_update_content_patches_existing_item
    fake = FakeZoteroClient.new(
      seed: { 'content_42_demo' => { key: 'ABCD1234', version: 7, data: {} } }
    )
    Brilliant::Zotero.client = fake
    with_zotero_env do
      assert Brilliant::Zotero.update_content(CONTENT_NOTIFICATION)
    end
    assert_equal ['content_42_demo'], fake.finds
    assert_empty fake.calls, 'an existing item must be updated, not re-created'
    assert_equal 1, fake.updates.length
    update = fake.updates.first
    assert_equal 'ABCD1234', update[:key]
    assert_equal 7, update[:version]
    assert_equal 'Content Updated: Week 3 slides (v2)', update[:item][:title]
  end

  def test_update_content_creates_once_when_item_absent
    # external_id is present but the library has no matching item yet (e.g.
    # content predating the integration). Create exactly once \u2014 still
    # duplicate-safe because we only create after an explicit lookup miss.
    fake = FakeZoteroClient.new(seed: {})
    Brilliant::Zotero.client = fake
    with_zotero_env do
      assert Brilliant::Zotero.update_content(CONTENT_NOTIFICATION)
    end
    assert_equal ['content_42_demo'], fake.finds
    assert_equal 1, fake.calls.length, 'absent item is created once'
    assert_empty fake.updates
  end

  def test_update_content_skips_missing_identifier
    # The "missing/unknown content identifier" case: no external_id to resolve
    # against. Must skip gracefully \u2014 no client traffic, no raise.
    fake = FakeZoteroClient.new(seed: {})
    Brilliant::Zotero.client = fake
    with_zotero_env do
      refute Brilliant::Zotero.update_content(
        CONTENT_NOTIFICATION.merge(external_id: '')
      )
    end
    assert_empty fake.finds
    assert_empty fake.calls
    assert_empty fake.updates
  end

  def test_update_content_skips_non_content_types
    fake = FakeZoteroClient.new(seed: {})
    Brilliant::Zotero.client = fake
    with_zotero_env do
      refute Brilliant::Zotero.update_content(
        CONTENT_NOTIFICATION.merge(notification_type: 'News')
      )
    end
    assert_empty fake.finds
    assert_empty fake.calls
    assert_empty fake.updates
  end

  def test_update_content_swallows_client_errors
    fake = FakeZoteroClient.new(raise_on_call: Brilliant::Zotero::Error.new('boom'))
    Brilliant::Zotero.client = fake
    with_zotero_env do
      refute Brilliant::Zotero.update_content(CONTENT_NOTIFICATION),
             'a client failure must not raise out of update_content'
    end
    assert_equal ['content_42_demo'], fake.finds
  end

  def test_update_content_nil_input_is_safe
    fake = FakeZoteroClient.new
    Brilliant::Zotero.client = fake
    with_zotero_env do
      refute Brilliant::Zotero.update_content(nil)
    end
    assert_empty fake.finds
    assert_empty fake.calls
    assert_empty fake.updates
  end
end
