require 'kramdown'

module Brilliant
  class TextProcessor
    def self.html_to_markdown(html)
      return "" if html.nil?
      
      # Handle D2L Rich Text Hash objects
      if html.is_a?(Hash)
        html = html["Html"] || html["Text"] || html[:Html] || html[:Text] || ""
      end
      
      # Handle stringified Ruby hashes (recovery from sync bugs)
      if html.is_a?(String) && html.start_with?("{") && html.include?("=>")
        if html =~ /"(?:Html|Text)"\s*=>\s*"(.*?)"/m
          html = $1
        elsif html =~ /:(?:Html|Text)\s*=>\s*"(.*?)"/m
          html = $1
        end
      end

      return "" if html.to_s.empty?
      
      # Very basic HTML -> Markdown conversion (extracted and improved)
      text = html.to_s.dup
      text.gsub!(/<br\s*\/?>/i, "\n")
      text.gsub!(/<\/p>/i, "\n\n")
      text.gsub!(/<p[^>]*>/i, "")
      text.gsub!(/<strong>(.*?)<\/strong>/i, '**\1**')
      text.gsub!(/<b>(.*?)<\/b>/i, '**\1**')
      text.gsub!(/<em>(.*?)<\/em>/i, '_\1_')
      text.gsub!(/<i>(.*?)<\/i>/i, '_\1_')
      text.gsub!(/<a\s+[^>]*href="([^"]*)"[^>]*>(.*?)<\/a>/i, '[\2](\1)')
      text.gsub!(/<ul[^>]*>/i, "\n")
      text.gsub!(/<\/ul>/i, "\n")
      text.gsub!(/<li[^>]*>/i, "\n* ")
      text.gsub!(/<\/li>/i, "")
      
      # Strip remaining tags
      text.gsub!(/<[^>]+>/, '')
      
      # Decode common entities
      text.gsub!("&nbsp;", " ")
      text.gsub!("&amp;", "&")
      text.gsub!("&lt;", "<")
      text.gsub!("&gt;", ">")
      text.gsub!("&quot;", "\"")

      text.strip
    end

    def self.render_markdown(text)
      return "" if text.nil? || (text.respond_to?(:empty?) && text.empty?)
      if text.is_a?(Hash)
        text = text['Html'] || text['Text'] || text[:Html] || text[:Text] || ""
      end
      Kramdown::Document.new(text.to_s).to_html
    end

    def self.fix_links(html, host = nil)
      return html if html.nil? || html.empty?
      
      # Prepend host to relative links if host is provided
      if host
        html = html.gsub(/href="(\/[^"]*)"/i, "href=\"https://#{host}\\1\"")
      end

      # Skip expensive auto-linking for very large strings
      return html if html.length > 20000

      # Auto-link plain text URLs while avoiding existing HTML tags
      tags = []
      temp_html = html.gsub(/<a\b[^>]*>.*?<\/a>|<[^>]+>/mi) do |match|
        tags << match
        "__T#{tags.size - 1}__"
      end

      url_regex = %r{https?://[^\s<"']+[^\s<"'. ,?!:)]}i
      temp_html.gsub!(url_regex) do |url|
        "<a href=\"#{url}\" target=\"_blank\">#{url}</a>"
      end

      tags.each_with_index do |tag, i|
        temp_html.sub!("__T#{i}__", tag)
      end

      temp_html
    end
  end
end
