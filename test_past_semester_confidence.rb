require_relative 'app'

raise 'Expected test database adapter' unless ActiveRecord::Base.connection.adapter_name =~ /sqlite/i

[Grade, Course, UserPreference].each(&:delete_all)

def assert_equal(expected, actual, message)
  return if expected == actual

  raise "#{message}: expected #{expected.inspect}, got #{actual.inspect}"
end

def assert_in_delta(expected, actual, delta, message)
  return if (expected - actual).abs <= delta

  raise "#{message}: expected #{actual.inspect} to be within #{delta} of #{expected.inspect}"
end

Course.create!(org_unit_id: 'past-weighted', name: 'Past Weighted', semester: 'Fall 2025')
Course.create!(org_unit_id: 'current-weighted', name: 'Current Weighted', semester: 'Spring 2026')
Course.create!(org_unit_id: 'past-points', name: 'Past Points', semester: 'Fall 2025')
Course.create!(org_unit_id: 'current-points', name: 'Current Points', semester: 'Spring 2026')

Grade.create!(course_id: 'past-weighted', brightspace_id: 'pw-graded', name: 'Used category', numerator: 60, denominator: 100, weight: 60)
Grade.create!(course_id: 'past-weighted', brightspace_id: 'pw-placeholder', name: 'Unused placeholder', weight: 40)
Grade.create!(course_id: 'current-weighted', brightspace_id: 'cw-graded', name: 'Used category', numerator: 60, denominator: 100, weight: 60)
Grade.create!(course_id: 'current-weighted', brightspace_id: 'cw-placeholder', name: 'Unused placeholder', weight: 40)

Grade.create!(course_id: 'past-points', brightspace_id: 'pp-graded', name: 'Posted grade', numerator: 60, denominator: 100)
Grade.create!(course_id: 'past-points', brightspace_id: 'pp-placeholder', name: 'Pending placeholder', denominator: 100, manually_marked_ungraded: true)
Grade.create!(course_id: 'current-points', brightspace_id: 'cp-graded', name: 'Posted grade', numerator: 60, denominator: 100)
Grade.create!(course_id: 'current-points', brightspace_id: 'cp-placeholder', name: 'Pending placeholder', denominator: 100, manually_marked_ungraded: true)

past_weighted = Grade.calculate_weighted_total('past-weighted')
current_weighted = Grade.calculate_weighted_total('current-weighted')
past_points = Grade.calculate_weighted_total('past-points')
current_points = Grade.calculate_weighted_total('current-points')

assert_equal(100.0, past_weighted[:confidence], 'past weighted course confidence should be final')
assert_in_delta(36.0, current_weighted[:confidence], 0.01, 'current weighted course confidence should remain partial')
assert_equal(100.0, past_points[:confidence], 'past points course confidence should be final')
assert_equal(30.0, current_points[:confidence], 'current points course confidence should remain partial')

summary = Brilliant::DashboardService.get_summary_data(UserPreference.create!(default_semester: 'Spring 2026'))
assert_equal('Spring 2026', summary[:semester], 'dashboard should still select requested semester')
raise 'dashboard GPA should remain numeric' unless summary[:overall_gpa].is_a?(Numeric)
raise 'dashboard max potential GPA should remain numeric' unless summary[:max_potential_gpa].is_a?(Numeric)

puts 'past semester confidence checks passed'
