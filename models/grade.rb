class Grade < ActiveRecord::Base
  validates :brightspace_id, presence: true, uniqueness: { scope: :course_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id

  def percentage
    return 0 if denominator.nil? || denominator == 0
    ((numerator.to_f / denominator.to_f) * 100).round(2)
  end

  def is_graded?
    # Brightspace often returns 0/X for ungraded items or items not yet submitted.
    # We check if numerator is not nil and displayed_grade is not empty or a placeholder
    !numerator.nil? && !(displayed_grade.nil? || displayed_grade.strip == "" || displayed_grade == "-")
  end

  def self.calculate_weighted_total(course_id)
    grades = where(course_id: course_id.to_s)
    
    # User Request: "Points determine weight (it is directly proportional)."
    # In a points-based system, the denominator IS the weight.
    
    graded_items = grades.select { |g| g.is_graded? && (g.denominator || 0) > 0 }
    
    if graded_items.empty?
      return { 
        score: 0, 
        confidence: 0, 
        total_points_earned: 0, 
        total_points_possible: 0,
        all_possible_points: grades.sum(:denominator) || 0
      } 
    end

    total_points_earned = graded_items.sum { |g| g.numerator || 0 }
    total_points_possible = graded_items.sum { |g| g.denominator || 0 }
    
    # All points in the syllabus (graded or not)
    all_possible_points = (grades.sum(:denominator) || 0).to_f
    
    current_score = (total_points_earned.to_f / total_points_possible.to_f) * 100
    
    # Confidence is what percent of the total points have been graded
    confidence = all_possible_points > 0 ? (total_points_possible.to_f / all_possible_points) * 100 : 0

    {
      score: current_score.round(2),
      confidence: confidence.round(1),
      total_points_earned: total_points_earned.round(2),
      total_points_possible: total_points_possible.round(2),
      all_possible_points: all_possible_points.round(2)
    }
  end

  def self.to_gpa(score)
    return 0.0 if score.nil?
    # Based on University of Maine System / USM GPA Scale
    if score >= 93
      4.00
    elsif score >= 90
      3.67
    elsif score >= 87
      3.33
    elsif score >= 83
      3.00
    elsif score >= 80
      2.67
    elsif score >= 77
      2.33
    elsif score >= 73
      2.00
    elsif score >= 70
      1.67
    elsif score >= 67
      1.33
    elsif score >= 63
      1.00
    elsif score >= 60
      0.67
    else
      0.00
    end
  end
end
