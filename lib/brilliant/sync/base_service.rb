module Brilliant
  module Sync
    class BaseService
      attr_reader :client

      def initialize(client)
        @client = client
      end

      protected

      def html_to_markdown(html)
        Brilliant::TextProcessor.html_to_markdown(html)
      end

      def publish_event(name, data)
        Brilliant::EventBus.publish(name, data)
      end

      def with_connection(&block)
        ActiveRecord::Base.connection_pool.with_connection(&block)
      end
    end
  end
end
