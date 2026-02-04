require 'sqlite3'
require 'json'
require 'digest'

db_path = 'db/development.sqlite3'
course_id = '446900'

# Data from browser scrape
data = [
    ["Discussion", "", "- / 20", "-%", ""],
    ["", "Introducing Me!", "- / 20", "- / -", "-%", ""],
    ["", "Ch 3 Discussion on Longevity", "- / 25", "- / -", "-%", ""],
    ["ReadingQuiz", "", "- / 20", "-%", ""],
    ["", "Ch 2 Quiz", "90 / 100", "0 / 0", "90 %", "2960109"], # Added ID manually for Ch 2 Quiz
    ["", "Ch3 Quiz", "- / 250", "- / -", "-%", ""],
    ["Activities", "", "- / 16", "-%", ""],
    ["", "Connect Orientation Videos", "- / 10", "- / -", "-%", ""],
    ["", "SmartBook 2.0 Overview", "- / 10", "- / -", "-%", ""],
    ["", "Ch3 Activity Growth Pattern", "- / 100", "- / -", "-%", ""],
    ["CumulativeQuiz", "", "0 / 0", "-%", ""],
    ["", "Chapter 1 (Bonus)", "100 / 100", "10", "", ""],
    ["", "Chapter 2: Biological Beginnings (Bonus)", "100 / 100", "10", "", ""],
    ["", "Ch 3 SMART BOOK (Bonus)", "- / 100", "- / -", "", ""],
    ["FinalExam", "", "- / 11", "-%", ""],
    ["FinalReflection", "", "- / 3", "-%", ""]
]

db = SQLite3::Database.new(db_path)

puts "Updating PSY-220 grades from scraped data..."

now = Time.now.strftime('%Y-%m-%d %H:%M:%S')

data.each do |row|
  name = row[0].empty? ? row[1] : row[0]
  next if name == "Grade Item" || name.empty?
  
  points_str = row[2]
  grade_str = row[4] || row[3] # Fallback
  
  # Manual ID mapping for known items
  # I'll use names to find existing IDs if possible
  
  existing_id = db.get_first_value("SELECT brightspace_id FROM grades WHERE course_id = ? AND name = ?", [course_id, name])
  
  brightspace_id = row[5] || existing_id || "scraped_#{Digest::MD5.hexdigest(name)[0..8]}"
  
  numerator = nil
  denominator = nil
  if points_str =~ /([\d\.]+)\s*\/\s*([\d\.]+)/
    numerator = $1.to_f
    denominator = $2.to_f
  elsif points_str =~ /\/\s*([\d\.]+)/
    denominator = $1.to_f
  end

  # Determine displayed grade
  displayed = grade_str == "-%" ? nil : grade_str
  
  db.execute(<<-SQL, [course_id, brightspace_id, name, displayed, numerator, denominator, now, now])
    INSERT INTO grades (course_id, brightspace_id, name, displayed_grade, numerator, denominator, updated_at, created_at)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
      name = excluded.name,
      displayed_grade = excluded.displayed_grade,
      numerator = COALESCE(excluded.numerator, numerator),
      denominator = COALESCE(excluded.denominator, denominator),
      updated_at = excluded.updated_at
SQL
  
  puts " - Updated: #{name} (#{displayed || 'Ungraded'})"
end

db.close
puts "Done."
