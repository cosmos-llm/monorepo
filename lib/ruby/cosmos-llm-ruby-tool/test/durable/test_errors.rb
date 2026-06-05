# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    class TestErrors < Minitest::Test
      def test_error_class_exists
        assert defined?(Tool::Error)
        assert Tool::Error.is_a?(Class)
      end

      def test_error_inherits_from_standard_error
        assert Tool::Error < StandardError
      end

      def test_validation_error_class_exists
        assert defined?(Tool::ValidationError)
        assert Tool::ValidationError.is_a?(Class)
      end

      def test_validation_error_inherits_from_error
        assert Tool::ValidationError < Tool::Error
        assert Tool::ValidationError < StandardError
      end

      def test_execution_error_class_exists
        assert defined?(Tool::ExecutionError)
        assert Tool::ExecutionError.is_a?(Class)
      end

      def test_execution_error_inherits_from_error
        assert Tool::ExecutionError < Tool::Error
        assert Tool::ExecutionError < StandardError
      end

      def test_tool_not_found_error_class_exists
        assert defined?(Tool::ToolNotFoundError)
        assert Tool::ToolNotFoundError.is_a?(Class)
      end

      def test_tool_not_found_error_inherits_from_error
        assert Tool::ToolNotFoundError < Tool::Error
        assert Tool::ToolNotFoundError < StandardError
      end

      def test_no_handler_error_class_exists
        assert defined?(Tool::NoHandlerError)
        assert Tool::NoHandlerError.is_a?(Class)
      end

      def test_no_handler_error_inherits_from_error
        assert Tool::NoHandlerError < Tool::Error
        assert Tool::NoHandlerError < StandardError
      end

      def test_error_can_be_raised_and_rescued
        error_raised = false
        begin
          raise Tool::Error, 'Test error message'
        rescue Tool::Error
          error_raised = true
        end
        assert error_raised, 'Tool::Error should be raisable and rescuable'
      end

      def test_validation_error_can_be_raised_and_rescued
        error_raised = false
        begin
          raise Tool::ValidationError, 'Validation failed'
        rescue Tool::ValidationError
          error_raised = true
        end
        assert error_raised, 'Tool::ValidationError should be raisable and rescuable'
      end

      def test_execution_error_can_be_raised_and_rescued
        error_raised = false
        begin
          raise Tool::ExecutionError, 'Execution failed'
        rescue Tool::ExecutionError
          error_raised = true
        end
        assert error_raised, 'Tool::ExecutionError should be raisable and rescuable'
      end

      def test_tool_not_found_error_can_be_raised_and_rescued
        error_raised = false
        begin
          raise Tool::ToolNotFoundError, 'Tool not found'
        rescue Tool::ToolNotFoundError
          error_raised = true
        end
        assert error_raised, 'Tool::ToolNotFoundError should be raisable and rescuable'
      end

      def test_no_handler_error_can_be_raised_and_rescued
        error_raised = false
        begin
          raise Tool::NoHandlerError, 'No handler defined'
        rescue Tool::NoHandlerError
          error_raised = true
        end
        assert error_raised, 'Tool::NoHandlerError should be raisable and rescuable'
      end

      def test_error_inheritance_allows_rescuing_base_error
        error_raised = false
        begin
          raise Tool::ValidationError, 'Validation failed'
        rescue Tool::Error
          error_raised = true
        end
        assert error_raised, 'Base Tool::Error should rescue subclasses'
      end

      def test_error_messages_are_preserved
        message = 'Custom error message'
        error = Tool::Error.new(message)
        assert_equal message, error.message
      end

      def test_validation_error_messages_are_preserved
        message = 'Parameter validation failed'
        error = Tool::ValidationError.new(message)
        assert_equal message, error.message
      end

      def test_execution_error_messages_are_preserved
        message = 'Tool execution encountered an error'
        error = Tool::ExecutionError.new(message)
        assert_equal message, error.message
      end

      def test_tool_not_found_error_messages_are_preserved
        message = 'Requested tool does not exist'
        error = Tool::ToolNotFoundError.new(message)
        assert_equal message, error.message
      end

      def test_no_handler_error_messages_are_preserved
        message = 'Tool has no execution handler'
        error = Tool::NoHandlerError.new(message)
        assert_equal message, error.message
      end

      def test_error_classes_are_instantiable
        error = Tool::Error.new
        assert_instance_of Tool::Error, error
      end

      def test_validation_error_is_instantiable
        error = Tool::ValidationError.new
        assert_instance_of Tool::ValidationError, error
      end

      def test_execution_error_is_instantiable
        error = Tool::ExecutionError.new
        assert_instance_of Tool::ExecutionError, error
      end

      def test_tool_not_found_error_is_instantiable
        error = Tool::ToolNotFoundError.new
        assert_instance_of Tool::ToolNotFoundError, error
      end

      def test_no_handler_error_is_instantiable
        error = Tool::NoHandlerError.new
        assert_instance_of Tool::NoHandlerError, error
      end

      def test_error_class_hierarchy
        # Ensure the inheritance chain is correct
        assert_equal StandardError, Tool::Error.superclass
        assert_equal Tool::Error, Tool::ValidationError.superclass
        assert_equal Tool::Error, Tool::ExecutionError.superclass
        assert_equal Tool::Error, Tool::ToolNotFoundError.superclass
        assert_equal Tool::Error, Tool::NoHandlerError.superclass
      end

      def test_error_classes_are_standard_error_subclasses
        # All error classes should be StandardError subclasses
        [Tool::Error, Tool::ValidationError, Tool::ExecutionError,
         Tool::ToolNotFoundError, Tool::NoHandlerError].each do |error_class|
          assert error_class < StandardError, "#{error_class} should be a StandardError subclass"
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
