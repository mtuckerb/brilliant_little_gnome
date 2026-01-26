require 'jwt'

module Brilliant
  module Auth
    def self.issue_token(user_uid)
      secret = UserPreference.current.jwt_secret
      payload = {
        uid: user_uid,
        exp: Time.now.to_i + 24 * 3600 # 24 hours
      }
      JWT.encode(payload, secret, 'HS256')
    end

    def self.decode_token(token)
      secret = UserPreference.current.jwt_secret
      begin
        decoded = JWT.decode(token, secret, true, { algorithm: 'HS256' })
        decoded[0]
      rescue JWT::DecodeError => e
        puts "JWT Decode Error: #{e.message}"
        nil
      end
    end
  end
end
