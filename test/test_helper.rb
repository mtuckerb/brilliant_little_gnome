# frozen_string_literal: true

# Minimal test bootstrap. We deliberately do NOT load app.rb / Sinatra so the
# Zotero unit tests stay fast and don't need the full Brightspace stack.

require 'minitest/autorun'
require 'minitest/spec'
require_relative '../lib/brilliant/zotero'

module ZoteroTestEnv
  # Helper: run a block with specific ZOTERO_* env vars, then restore.
  def with_env(overrides)
    backup = {}
    overrides.each_key { |k| backup[k] = ENV[k] }
    overrides.each { |k, v| ENV[k] = v }
    yield
  ensure
    backup.each { |k, v| ENV[k] = v }
  end

  # Resets the memoised Zotero client between tests.
  def reset_zotero_client!
    Brilliant::Zotero.client = nil
  end
end

# A spy that records every Zotero client call so tests can assert against it.
#
#   calls   - items passed to post_item (creates)
#   updates - { key:, version:, item: } hashes passed to update_item
#   finds   - external_ids passed to find_item_by_external_id
#
# `seed` maps external_id => { key:, version:, data: } so find_item_by_external_id
# can return a pre-existing library item for the update path.
class FakeZoteroClient
  attr_reader :calls, :updates, :finds

  def initialize(raise_on_call: nil, seed: {})
    @calls = []
    @updates = []
    @finds = []
    @seed = seed
    @raise_on_call = raise_on_call
  end

  def post_item(item)
    @calls << item
    raise @raise_on_call if @raise_on_call

    Struct.new(:code, :body).new('200', '{}')
  end

  def find_item_by_external_id(external_id)
    @finds << external_id
    raise @raise_on_call if @raise_on_call

    @seed[external_id]
  end

  def update_item(item_key, version, item)
    @updates << { key: item_key, version: version, item: item }
    raise @raise_on_call if @raise_on_call

    Struct.new(:code, :body).new('204', '')
  end
end
