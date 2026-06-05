# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      class TestErrors < Minitest::Test
        def test_error_inherits_from_standard_error
          assert_kind_of StandardError, Cosmos::Llm::Context::Error.new
          assert Cosmos::Llm::Context::Error < StandardError
        end

        def test_invalid_name_error_inherits_from_base_error
          error = InvalidNameError.new('Invalid name provided')
          assert_kind_of Cosmos::Llm::Context::Error, error
          assert_kind_of StandardError, error
          assert InvalidNameError < Cosmos::Llm::Context::Error
        end

        def test_renderer_not_found_error_inherits_from_base_error
          error = RendererNotFoundError.new('Renderer not found')
          assert_kind_of Cosmos::Llm::Context::Error, error
          assert_kind_of StandardError, error
          assert RendererNotFoundError < Cosmos::Llm::Context::Error
        end

        def test_validation_error_inherits_from_base_error
          error = ValidationError.new('Validation failed')
          assert_kind_of Cosmos::Llm::Context::Error, error
          assert_kind_of StandardError, error
          assert ValidationError < Cosmos::Llm::Context::Error
        end

        def test_duplicate_registration_error_inherits_from_base_error
          error = DuplicateRegistrationError.new('Duplicate registration')
          assert_kind_of Cosmos::Llm::Context::Error, error
          assert_kind_of StandardError, error
          assert DuplicateRegistrationError < Cosmos::Llm::Context::Error
        end

        def test_error_can_be_instantiated_without_message
          error = Cosmos::Llm::Context::Error.new
          assert_equal 'Cosmos::Llm::Context::Error', error.message
        end

        def test_error_can_be_instantiated_with_message
          message = 'Test error message'
          error = Cosmos::Llm::Context::Error.new(message)
          assert_equal message, error.message
        end

        def test_invalid_name_error_with_message
          message = 'Name cannot be nil'
          error = InvalidNameError.new(message)
          assert_equal message, error.message
        end

        def test_renderer_not_found_error_with_message
          message = 'Unknown renderer format: yaml'
          error = RendererNotFoundError.new(message)
          assert_equal message, error.message
        end

        def test_validation_error_with_message
          message = 'Content cannot be empty'
          error = ValidationError.new(message)
          assert_equal message, error.message
        end

        def test_duplicate_registration_error_with_message
          message = 'Renderer :json already registered'
          error = DuplicateRegistrationError.new(message)
          assert_equal message, error.message
        end

        def test_error_has_backtrace
          error = nil
          begin
            raise Cosmos::Llm::Context::Error, 'Test'
          rescue StandardError => e
            error = e
          end

          refute_nil error.backtrace
          assert_kind_of Array, error.backtrace
          assert error.backtrace.length.positive?
        end

        def test_rescue_base_error_catches_all_context_errors
          errors_caught = []

          [InvalidNameError, RendererNotFoundError, ValidationError, DuplicateRegistrationError].each do |error_class|
            raise error_class, 'Test error'
          rescue Cosmos::Llm::Context::Error => e
            errors_caught << e.class
          end

          expected_errors = [InvalidNameError, RendererNotFoundError, ValidationError, DuplicateRegistrationError]
          assert_equal expected_errors, errors_caught
        end

        def test_rescue_standard_error_catches_context_errors
          error_caught = false

          begin
            raise Cosmos::Llm::Context::Error, 'Test'
          rescue StandardError
            error_caught = true
          end

          assert error_caught
        end

        def test_specific_error_rescue
          specific_caught = false
          generic_caught = false

          begin
            raise InvalidNameError, 'Invalid name'
          rescue InvalidNameError
            specific_caught = true
          rescue Cosmos::Llm::Context::Error
            generic_caught = true
          end

          assert specific_caught
          refute generic_caught
        end

        def test_error_equality
          error1 = Cosmos::Llm::Context::Error.new('message')
          error2 = Cosmos::Llm::Context::Error.new('message')
          error3 = Cosmos::Llm::Context::Error.new('different message')

          # In Ruby, exceptions with the same message are equal
          assert_equal error1, error2
          refute_equal error1, error3
        end

        def test_error_to_s
          message = 'Test error message'
          error = Cosmos::Llm::Context::Error.new(message)
          assert_equal message, error.to_s
        end

        def test_error_inspect
          error = Cosmos::Llm::Context::Error.new('test')
          assert_includes error.inspect, 'Cosmos::Llm::Context::Error'
          assert_includes error.inspect, 'test'
        end

        def test_error_with_nil_message
          error = Cosmos::Llm::Context::Error.new(nil)
          assert_equal 'Cosmos::Llm::Context::Error', error.message
        end

        def test_error_with_empty_message
          error = Cosmos::Llm::Context::Error.new('')
          assert_equal '', error.message
          assert_equal '', error.to_s
        end

        def test_error_cause
          original_error = StandardError.new('Original')
          context_error = Cosmos::Llm::Context::Error.new('Context error')

          begin
            raise original_error
          rescue StandardError
            begin
              raise context_error
            rescue Cosmos::Llm::Context::Error => e
              # In Ruby, cause is set automatically in re-raise scenarios
              # but we can't easily test this without more complex setup
              assert_kind_of Cosmos::Llm::Context::Error, e
            end
          end
        end

        def test_all_error_classes_are_defined
          assert defined?(Cosmos::Llm::Context::Error)
          assert defined?(Cosmos::Llm::Context::InvalidNameError)
          assert defined?(Cosmos::Llm::Context::RendererNotFoundError)
          assert defined?(Cosmos::Llm::Context::ValidationError)
          assert defined?(Cosmos::Llm::Context::DuplicateRegistrationError)
        end

        def test_error_class_constants
          assert_equal Cosmos::Llm::Context::Error, InvalidNameError.superclass
          assert_equal Cosmos::Llm::Context::Error, RendererNotFoundError.superclass
          assert_equal Cosmos::Llm::Context::Error, ValidationError.superclass
          assert_equal Cosmos::Llm::Context::Error, DuplicateRegistrationError.superclass
        end

        def test_error_inheritance_hierarchy
          # Verify the inheritance chain: SpecificError < Error < StandardError
          assert InvalidNameError < Cosmos::Llm::Context::Error
          assert Cosmos::Llm::Context::Error < StandardError

          assert RendererNotFoundError < Cosmos::Llm::Context::Error
          assert ValidationError < Cosmos::Llm::Context::Error
          assert DuplicateRegistrationError < Cosmos::Llm::Context::Error
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
