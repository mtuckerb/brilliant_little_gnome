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

# A spy that records every push_item call so tests can assert against it.
class FakeZoteroClient
  attr_reader :calls

  def initialize(raise_on_call: nil)
    @calls = []
    @raise_on_call = raise_on_call
  end

  def post_item(item)
    @calls << item
    raise @raise_on_call if @raise_on_call

    Struct.new(:code, :body).new('200', '{}')
  end
end
