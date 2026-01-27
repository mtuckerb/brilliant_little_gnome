class Grade < ActiveRecord::Base
  include HasUserIdentity
  validates :brightspace_id, presence: true, uniqueness: { scope: :course_id }
  belongs_to :course, foreign_key: :course_id, primary_key: :org_unit_id

  def percentage
    return 0 if denominator.nil? || denominator == 0
    ((numerator.to_f / denominator.to_f) * 100).round(2)
  end

  def is_graded?
    !numerator.nil? && !(displayed_grade.nil? || displayed_grade.strip == '' || displayed_grade == '-')
  end

  def self.calculate_weighted_total(course_id)
    # Simple request-local memoization
    cache_key = "weighted_total_#{course_id}"
    return Thread.current[cache_key] if Thread.current[cache_key]

    Thread.current[cache_key] = _calculate_weighted_total(course_id)
  end

  def self._calculate_weighted_total(course_id)
    grades = where(course_id: course_id.to_s)
    graded_items = grades.select { |g| g.is_graded? && (g.denominator || 0) > 0 }
    
    if graded_items.empty?
      all_possible_points = (grades.where(is_extra_credit: false).sum(:denominator) || 0).to_f
      return { 
        score: 0, 
        confidence: 0, 
        total_points_earned: 0, 
        total_points_possible: 0,
        all_possible_points: all_possible_points,
        remaining_points: all_possible_points,
        max_potential_score: 100.0,
        required_avg: nil
      } 
    end

    total_points_earned = graded_items.sum { |g| g.numerator || 0 }
    total_points_possible = graded_items.reject(&:is_extra_credit).sum { |g| g.denominator || 0 }
    all_possible_points = (grades.where(is_extra_credit: false).sum(:denominator) || 0).to_f
    
    current_score = total_points_possible > 0 ? (total_points_earned.to_f / total_points_possible.to_f) * 100 : 100.0
    base_confidence = all_possible_points > 0 ? (total_points_possible.to_f / all_possible_points) * 100 : 0
    
    if all_possible_points >= 100
      confidence = base_confidence
    else
      item_count_multiplier = [graded_items.count / 3.0, 1.0].min
      confidence = base_confidence * item_count_multiplier
    end

    remaining_points = all_possible_points - total_points_possible
    max_potential_score = all_possible_points > 0 ? ((total_points_earned + remaining_points) / all_possible_points) * 100 : 100.0

    target_grade = Course.find_by(org_unit_id: course_id)&.target_grade || 93.0
    required_points_to_hit_target = (target_grade / 100.0) * all_possible_points
    points_needed = [required_points_to_hit_target - total_points_earned, 0].max
    
    required_avg = remaining_points > 0 ? (points_needed / remaining_points) * 100 : nil
    is_impossible = required_avg && required_avg > 100

    {
      score: current_score.round(2),
      confidence: confidence.round(1),
      total_points_earned: total_points_earned.round(2),
      total_points_possible: total_points_possible.round(2),
      all_possible_points: all_possible_points.round(2),
      remaining_points: remaining_points.round(2),
      max_potential_score: max_potential_score.round(2),
      target_grade: target_grade,
      required_avg: required_avg ? required_avg.round(2) : nil,
      is_impossible: is_impossible,
      points_needed: points_needed.round(2),
      item_count: graded_items.count
    }
  end

  def self.to_gpa(score)
    return 0.0 if score.nil?
    if score >= 93 then 4.00
    elsif score >= 90 then 3.67
    elsif score >= 87 then 3.33
    elsif score >= 83 then 3.00
    elsif score >= 80 then 2.67
    elsif score >= 77 then 2.33
    elsif score >= 73 then 2.00
    elsif score >= 70 then 1.67
    elsif score >= 67 then 1.33
    elsif score >= 63 then 1.00
    elsif score >= 60 then 0.67
    else 0.00
    end
  end
end
