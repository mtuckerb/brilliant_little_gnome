module Brilliant
  module Sync
    class DiscussionService < BaseService
      def sync(course_id, forums)
        with_connection do
          ActiveRecord::Base.transaction do
            client.ensure_array(forums).each do |f|
              forum = DiscussionForum.find_or_initialize_by(brightspace_id: f['ForumId'].to_s, course_id: course_id.to_s)
              forum.name = f['Name']
              forum.description = html_to_markdown(f['Description'])
              forum.save!
            end
          end
        end

        all_topics_path = "/d2l/api/le/1.40/#{course_id}/discussions/topics/"
        topics_raw = client.do_get(all_topics_path)
        if topics_raw
          sync_topics(course_id, nil, client.ensure_array(topics_raw))
        else
          client.ensure_array(forums).each do |f|
            topics = client.ensure_array(client.get_discussion_topics(course_id, f['ForumId']))
            sync_topics(course_id, f['ForumId'], topics)
          end
        end
      end

      def sync_topics(course_id, forum_id, topics)
        with_connection do
          ActiveRecord::Base.transaction do
            topics.each_with_index do |t, index|
              fid = forum_id || t['ForumId']
              next unless fid

              topic = DiscussionTopic.find_or_initialize_by(brightspace_id: t['TopicId'].to_s, forum_id: fid.to_s)
              topic.course_id = course_id.to_s
              topic.name = t['Name']
              topic.description = html_to_markdown(t['Description'])
              topic.sort_order = index
              topic.thread_count = t['ThreadCount'] if t['ThreadCount']
              topic.post_count = t['PostCount'] if t['PostCount']
              topic.last_post_date = Time.zone.parse(t['LastPostDate']) rescue nil if t['LastPostDate']
              topic.save!
            end
          end
        end
      end
    end
  end
end
