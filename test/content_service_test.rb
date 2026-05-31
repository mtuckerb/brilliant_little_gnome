# frozen_string_literal: true

require 'minitest/autorun'
require 'active_record'
require 'json'

require_relative '../models/course'
require_relative '../models/content_module'
require_relative '../models/content_item'
require_relative '../lib/brilliant/text_processor'
require_relative '../lib/brilliant/sync/base_service'
require_relative '../lib/brilliant/sync/content_service'

class ContentServiceTest < Minitest::Test
  class FakeClient
    attr_reader :overview_calls, :toc_calls

    def initialize(tocs:, overviews:)
      @tocs = tocs
      @overviews = overviews
      @toc_calls = []
      @overview_calls = []
    end

    def get_toc(course_id)
      @toc_calls << course_id
      @tocs.fetch(course_id)
    end

    def get_overview(course_id, force_refresh: false)
      @overview_calls << [course_id, force_refresh]
      @overviews.fetch(course_id)
    end
  end

  def setup
    ActiveRecord::Base.establish_connection(adapter: 'sqlite3', database: ':memory:')
    load_schema
  end

  def teardown
    ActiveRecord::Base.connection.disconnect! if ActiveRecord::Base.connected?
  end

  def test_sync_content_for_course_refreshes_overview_and_replaces_stale_content
    old_overview = { 'Text' => { 'Html' => '<a href="old-url">Old</a>' } }.to_json
    course = Course.create!(org_unit_id: '101', name: 'Biology', user_id: 'user-1', overview_raw: old_overview)
    stale_module = ContentModule.create!(course_id: '101', brightspace_id: 'old-module', title: 'Old module', user_id: 'user-1')
    ContentItem.create!(module_id: stale_module.brightspace_id, brightspace_id: 'old-topic', title: 'Old topic', url: 'old-url', user_id: 'user-1')

    overview = { 'Text' => { 'Html' => '<a href="fresh-url">Fresh</a>' }, 'Attachments' => [{ 'Url' => 'fresh-attachment' }] }
    toc = {
      'Modules' => [
        {
          'ModuleId' => 'new-module',
          'Title' => 'New module',
          'Description' => { 'Html' => '<p>Intro</p>' },
          'SortOrder' => 1,
          'Topics' => [
            {
              'TopicId' => 'new-topic',
              'Title' => 'New topic',
              'Type' => 1,
              'Url' => 'fresh-url',
              'IsHidden' => false,
              'SortOrder' => 1,
              'Attachments' => [{ 'Url' => 'fresh-topic-attachment' }]
            }
          ]
        }
      ]
    }
    client = FakeClient.new(tocs: { '101' => toc }, overviews: { '101' => overview })

    Brilliant::Sync::ContentService.new(client).sync_content_for_course(course)

    assert_equal [['101', true]], client.overview_calls
    assert_equal overview, JSON.parse(course.reload.overview_raw)
    assert_nil ContentModule.find_by(brightspace_id: 'old-module')
    assert_nil ContentItem.find_by(brightspace_id: 'old-topic')
    assert_equal 'fresh-url', ContentItem.find_by(brightspace_id: 'new-topic').url
  end

  def test_sync_all_content_refreshes_overview_for_each_course
    course_one = Course.create!(org_unit_id: '201', name: 'Math', user_id: 'user-1', overview_raw: stale_overview_json)
    course_two = Course.create!(org_unit_id: '202', name: 'History', user_id: 'user-1', overview_raw: stale_overview_json)
    client = FakeClient.new(
      tocs: {
        '201' => empty_toc,
        '202' => empty_toc
      },
      overviews: {
        '201' => { 'Text' => { 'Html' => 'fresh one' } },
        '202' => { 'Text' => { 'Html' => 'fresh two' } }
      }
    )

    Brilliant::Sync::ContentService.new(client).sync_all_content

    assert_equal [['201', true], ['202', true]], client.overview_calls
    assert_equal 'fresh one', JSON.parse(course_one.reload.overview_raw).dig('Text', 'Html')
    assert_equal 'fresh two', JSON.parse(course_two.reload.overview_raw).dig('Text', 'Html')
  end

  private

  def stale_overview_json
    { 'Text' => { 'Html' => 'stale' } }.to_json
  end

  def empty_toc
    { 'Modules' => [] }
  end

  def load_schema
    ActiveRecord::Schema.define do
      create_table :courses, force: true do |t|
        t.string :org_unit_id
        t.string :name
        t.string :custom_name
        t.string :code
        t.string :semester
        t.datetime :last_accessed_at
        t.boolean :is_pinned, default: false
        t.integer :units, default: 3
        t.float :target_grade, default: 93.0
        t.string :banner_url
        t.string :custom_color
        t.integer :end_of_week_day, default: 0
        t.string :user_id
        t.text :overview_raw
        t.string :status, default: 'active'
        t.datetime :dropped_at
        t.integer :sort_order, default: 0
        t.timestamps
      end

      create_table :content_modules, force: true do |t|
        t.string :course_id
        t.string :brightspace_id
        t.string :title
        t.text :description
        t.integer :sort_order
        t.string :parent_id
        t.string :user_id
        t.timestamps
      end

      create_table :content_items, force: true do |t|
        t.string :module_id
        t.string :brightspace_id
        t.string :title
        t.string :item_type
        t.string :url
        t.boolean :is_hidden, default: false
        t.integer :sort_order
        t.text :attachments
        t.string :user_id
        t.timestamps
      end
    end
  end
end
