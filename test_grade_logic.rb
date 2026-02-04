require_relative 'app'

# Find or create a test grade
course_id = "test_course"
bs_id = "test_grade_1"

grade = Grade.find_or_initialize_by(course_id: course_id, brightspace_id: bs_id)
grade.update!(
  name: "Test Grade",
  numerator: 0,
  denominator: 100,
  manually_marked_ungraded: true
)

puts "Initial state: numerator=#{grade.numerator}, manually_marked_ungraded=#{grade.manually_marked_ungraded}, is_graded?=#{grade.is_graded?}"

# Simulate sync with 0
service = Brilliant::Sync::GradeService.new($client)
service.sync(course_id, [{
  'Identifier' => bs_id,
  'GradeObjectName' => "Test Grade",
  'DisplayedGrade' => "0 / 100",
  'Numerator' => 0,
  'Denominator' => 100
}])

grade.reload
puts "After sync with 0: numerator=#{grade.numerator}, manually_marked_ungraded=#{grade.manually_marked_ungraded}, is_graded?=#{grade.is_graded?}"

# Simulate sync with 85
service.sync(course_id, [{
  'Identifier' => bs_id,
  'GradeObjectName' => "Test Grade",
  'DisplayedGrade' => "85 / 100",
  'Numerator' => 85,
  'Denominator' => 100
}])

grade.reload
puts "After sync with 85: numerator=#{grade.numerator}, manually_marked_ungraded=#{grade.manually_marked_ungraded}, is_graded?=#{grade.is_graded?}"

# Cleanup
grade.destroy
puts "Done."
