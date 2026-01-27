require 'thread'

module Brilliant
  class EventBus
    @subscribers = []
    @lock = Mutex.new

    def self.subscribe(queue)
      @lock.synchronize { @subscribers << queue }
    end

    def self.unsubscribe(queue)
      @lock.synchronize { @subscribers.delete(queue) }
    end

    def self.publish(event, data)
      @lock.synchronize do
        @subscribers.each { |q| q << { event: event, data: data } }
      end
    end
  end
end
