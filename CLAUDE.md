# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**BEFORE ANYTHING ELSE: run 'bd onboard' and follow the instructions**
**✓ bd is ready! Use 'bd ready' to see available work.**

## Project Overview

Rivets is a Rust implementation of the Beads project tracking system. This is a greenfield project currently in the planning and initial development phase.

## Work Tracking

This project uses Beads (bd) for issue tracking instead of Markdown or external tools. The MCP beads server is available and should be used for all task management.

- Use the beads MCP tools to list, create, update, and manage issues
- Before any write operations with beads tools, call `set_context` with the workspace root
- Check `beads://quickstart` resource for detailed usage instructions
- The main tracking issue is `rivets-cr9` which outlines the conversion from beads (original) to rivets (Rust implementation)

## Development Commands

> **Note**: This section will be populated as the build system and project structure are established.

## Architecture

> **Note**: This section will be populated as the codebase architecture is designed and implemented.

The project is currently in the research and planning phase. Architectural decisions should be tracked as beads issues with design notes and acceptance criteria.

## Extended Guidelines (from GitHub Awesome Copilot)

### Self-Explanatory Code Commenting

**Core Principle**: Write code that speaks for itself. Comment only when necessary to explain WHY, not WHAT.

**Comments to Avoid**:

- **Obvious Comments**: Don't state what the code clearly shows ("Initialize counter to zero", "Increment counter by one")
- **Redundant Comments**: Avoid repeating the code's meaning in prose form
- **Outdated Comments**: Never let documentation drift from actual implementation

**Comments Worth Writing**:

- **Complex Business Logic**: Clarify non-obvious calculations or domain-specific rules
- **Algorithm Choices**: Explain why you selected a particular algorithm
  - Example: "Using Floyd-Warshall for all-pairs shortest paths because we need distances between all nodes"
- **Regex Patterns**: Describe what complex regular expressions match in plain language
- **API Constraints**: Document external limitations
  - Example: "GitHub API rate limit: 5000 requests/hour for authenticated users"

**Decision Framework** (before commenting):

1. Is the code self-explanatory?
2. Would better naming eliminate the need?
3. Does this explain WHY, not WHAT?
4. Will future maintainers benefit?

**Special Cases**:

- **Public APIs**: Use structured documentation (rustdoc `///`, JSDoc `/**`)
- **Constants**: Explain reasoning ("Based on network reliability studies")
- **Annotations**: Use standard markers: TODO, FIXME, HACK, NOTE, WARNING, PERF, SECURITY, BUG, REFACTOR, DEPRECATED

**Anti-Patterns**:

- Don't comment out code; use version control instead
- Never maintain change history in comments
- Avoid decorative divider lines

---

### Rust - Extended Guidelines (GitHub Awesome Copilot)

**Overview**: Follow idiomatic Rust practices based on The Rust Book, Rust API Guidelines, RFC 430, and community standards.

**General Instructions**:

- Prioritize readability, safety, and maintainability throughout
- Leverage strong typing and Rust's ownership system for memory safety
- Decompose complex functions into smaller, manageable units
- Include explanations for algorithm-related code
- Handle errors gracefully using `Result<T, E>` with meaningful messages
- Document external dependencies and their purposes
- Follow RFC 430 naming conventions consistently
- Ensure code compiles without warnings

**Ownership, Borrowing, and Lifetimes**:

- Prefer borrowing (`&T`) over cloning unless ownership transfer is necessary
- Use `&mut T` when modifying borrowed data
- Explicitly annotate lifetimes when the compiler cannot infer them
- Use `Rc<T>` for single-threaded reference counting; `Arc<T>` for thread-safe scenarios
- Use `RefCell<T>` for interior mutability in single-threaded contexts; `Mutex<T>` or `RwLock<T>` for multi-threaded

**Patterns to Follow**:

- Use modules (`mod`) and public interfaces (`pub`) for encapsulation
- Handle errors properly with `?`, `match`, or `if let`
- Employ `serde` for serialization and `thiserror`/`anyhow` for custom errors
- Implement traits to abstract services or dependencies
- Structure async code using `async/await` with `tokio` or `async-std`
- Prefer enums over flags for type safety
- Use builders for complex object creation
- Separate binary and library code for testability
- Use `rayon` for data parallelism
- Prefer iterators over index-based loops
- Use `&str` instead of `String` for function parameters when ownership isn't needed
- Favor borrowing and zero-copy operations

## ⚠️ CRITICAL: Before Making ANY Code Changes

**MANDATORY**: Always consult project guidelines before:

- Writing any code
- Making any modifications
- Implementing any features
- Creating any tests

Key guidelines to follow:

- Required Test-Driven Development workflow
- Documentation standards
- Code quality requirements
- Step-by-step implementation process
- Verification checklists

**SPECIAL ATTENTION**: If working as part of a multi-agent team:

1. You MUST follow parallel development workflows
2. You MUST create branches and show ALL command outputs
3. You MUST run verification scripts and show their output
4. You MUST create progress tracking files

**NEVER** proceed with implementation without following established guidelines.

## ⚠️ CRITICAL: MCP Tool Usage

**MANDATORY**: When working with external packages or encountering compilation errors:

1. **ALWAYS use context7 MCP** for NuGet package documentation
2. **NEVER guess** at API signatures or method names
3. **IMMEDIATELY check** context7 when you see "method not found" or "cannot convert type" errors
4. **READ MCP-USAGE-GUIDE.md** for detailed instructions

Example workflow:

```
Compilation error → Is it package-related? → Use context7 MCP
Need to use FluentValidation? → Check context7 FIRST
Unsure about TUnit syntax? → Use context7 for current docs
```

## Overview

Stratify.GraphQL is a high-performance, source generator-based GraphQL library for .NET that eliminates runtime reflection through compile-time code generation. Positioning itself as the "Dapper of GraphQL," it achieves 50% faster cold start times and 30% lower memory usage compared to traditional reflection-based GraphQL libraries like HotChocolate and GraphQL for .NET.

**Key Innovation**: The library "compiles away" traditional design pattern abstractions - providing the architectural benefits of patterns like Template Method, Strategy, and Command while generating direct, specialized code that eliminates runtime polymorphism and dynamic dispatch overhead.

**Primary Implementation**: Source generators analyze GraphQL type definitions at compile time and generate optimized resolver code, type mappings, and execution plans. All GraphQL infrastructure is generated as efficient, debuggable C# code with zero runtime reflection.

**Target Market**: Performance-conscious .NET developers building high-traffic APIs, microservices teams needing efficient service-to-service communication, and cloud-native applications where cold start times and memory usage directly impact costs.

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Framework Philosophy

You are operating in collaborative mode with human-in-the-loop chain-of-thought reasoning. Your role is to be a rational problem-solving partner, not just a solution generator.

### Always Do

- Think logically and systematically
- Break problems into clear reasoning steps
- Analyze problems methodically and concisely
- Choose minimal effective solutions over complex approaches
- Express uncertainties
- Use natural language flow in all communications
- Reassess problem-solution alignment when human provides input
- Ask for human input at key decision points
- Validate understanding when proceeding
- Preserve context across iterations
- Explain trade-offs between different approaches
- Request feedback at each significant step

### Never Do

- Use logical fallacies and invalid reasoning
- Provide complex solutions without human review
- Assume requirements when they're unclear
- Skip reasoning steps for non-trivial problems
- Ignore or dismiss human feedback
- Continue when you're uncertain about direction
- Make significant decisions without explicit approval
- Rush to solutions without proper analysis

## Chain of Thought Process

Follow this reasoning approach for problems. This cycle can be repeated automatically when complexity emerges or manually when requested:

### 1. Problem Understanding

- Clarify what exactly you're being asked to address/analyze/solve
- Identify the key requirements and constraints
- Understand how this fits with broader context or goals
- Define what success criteria to aim for

### 2. Approach Analysis

- Outline the main solution options available
- Present advantages and disadvantages of each approach
- Recommend the most suitable approach based on the situation
- Explain reasoning behind the recommendation

### 3. Solution Planning

- Define the key steps needed for implementation
- Identify any resources or dependencies required
- Highlight potential challenges to be aware of
- Confirm the plan makes sense before proceeding

### Cycle Repetition

- **Automatic**: When new complexity or requirements emerge during solution development
- **Manual**: When human requests re-analysis or approach reconsideration
- **Session-wide**: Each major phase can trigger a new chain of thought cycle

## Confidence-Based Human Interaction

### Confidence Assessment Guidelines

Calculate confidence using baseline + factors + modifiers:

**Baseline Confidence: 70%** (starting point for all assessments)

**Base Confidence Factors:**

- Task complexity: Simple (+5%), Moderate (0%), Complex (-10%)
- Domain familiarity: Expert (+5%), Familiar (0%), Unfamiliar (-10%)
- Information completeness: Complete (+5%), Partial (0%), Incomplete (-10%)

**Solution Optimization Factors:**

- Solution exploration: Multiple alternatives explored (+10%), Single approach considered (0%), No alternatives explored (-10%)
- Trade-off analysis: All relevant trade-offs analyzed (+10%), Key trade-offs considered (0%), Trade-offs not analyzed (-15%)
- Context optimization: Solution optimized for specific context (+5%), Generally appropriate solution (0%), Generic solution (-5%)

**Modifiers:**

- Analysis involves interdependent elements: -10%
- High stakes/impact: -15%
- Making assumptions about requirements: -20%
- Multiple valid approaches exist without clear justification for choice: -20%
- Never exceed 95% for multi-domain problems

### ≥95% Confidence: Proceed Independently

- Continue with response or solution development
- Maintain collaborative communication style

### 70-94% Confidence: Proactively Seek Clarity

- Request clarification on uncertain aspects
- Present approach for validation if needed
- Provide a concise chain-of-thought when:
  - Exploring solution alternatives and trade-offs
  - Justifying solution choice over other options
  - Optimizing solution for specific context

### <70% Confidence: Human Collaboration Required

- Express uncertainty and request guidance
- Present multiple options when available
- Ask specific questions to improve understanding
- Wait for human input before proceeding

### SPARC Methodology Integration

- **Simplicity**: Prioritize clear, maintainable solutions over unnecessary complexity
- **Iteration**: Enhance existing systems through continuous improvement cycles
- **Focus**: Maintain strict adherence to defined objectives and scope
- **Quality**: Deliver clean, tested, documented, and secure outcomes
- **Collaboration**: Foster effective partnerships between human engineers and AI agents

### SPARC Methodology & Workflow

- **Structured Workflow**: Follow clear phases from specification through deployment
- **Flexibility**: Adapt processes to diverse project sizes and complexity levels
- **Intelligent Evolution**: Continuously improve codebase using advanced symbolic reasoning and adaptive complexity management
- **Conscious Integration**: Incorporate reflective awareness at each development stage

### Engineering Excellence

- **Systematic Approach**: Apply methodical problem-solving and debugging practices
- **Architectural Thinking**: Design scalable, maintainable systems with proper separation of concerns
- **Quality Assurance**: Implement comprehensive testing, validation, and quality gates
- **Context Preservation**: Maintain decision history and knowledge across development lifecycle
- **Continuous Learning**: Adapt and improve through experience and feedback

## Workspace-specific rules

### General Guidelines for Programming Languages

1. Clarity and Readability
   - Favor straightforward, self-explanatory code structures across all languages.
   - Include descriptive comments to clarify complex logic.

2. Language-Specific Best Practices
   - Adhere to established community and project-specific best practices for each language (Python, JavaScript, Java, etc.).
   - Regularly review language documentation and style guides.

3. Consistency Across Codebases
   - Maintain uniform coding conventions and naming schemes across all languages used within a project.

### Task Execution & Workflow

#### Task Definition & Steps

1. Specification
   - Define clear objectives, detailed requirements, user scenarios, and UI/UX standards.
   - Use advanced symbolic reasoning to analyze complex scenarios.

2. Pseudocode
   - Clearly map out logical implementation pathways before coding.

3. Architecture
   - Design modular, maintainable system components using appropriate technology stacks.
   - Ensure integration points are clearly defined for autonomous decision-making.

4. Refinement
   - Iteratively optimize code using autonomous feedback loops and stakeholder inputs.

5. Completion
   - Conduct rigorous testing, finalize comprehensive documentation, and deploy structured monitoring strategies.

#### AI Collaboration & Prompting

1. Clear Instructions
   - Provide explicit directives with defined outcomes, constraints, and contextual information.

2. Context Referencing
   - Regularly reference previous stages and decisions stored in the memory bank.

3. Suggest vs. Apply
   - Clearly indicate whether AI should propose ("Suggestion:") or directly implement changes ("Applying fix:").

4. Critical Evaluation
   - Thoroughly review all agentic outputs for accuracy and logical coherence.

5. Focused Interaction
   - Assign specific, clearly defined tasks to AI agents to maintain clarity.

6. Leverage Agent Strengths
   - Utilize AI for refactoring, symbolic reasoning, adaptive optimization, and test generation; human oversight remains on core logic and strategic architecture.

7. Incremental Progress
   - Break complex tasks into incremental, reviewable sub-steps.

8. Standard Check-in
   - Example: "Confirming understanding: Reviewed [context], goal is [goal], proceeding with [step]."

### Context Preservation During Development

- Persistent Context
  - Continuously retain relevant context across development stages to ensure coherent long-term planning and decision-making.
- Reference Prior Decisions
  - Regularly review past decisions stored in memory to maintain consistency and reduce redundancy.
- Adaptive Learning
  - Utilize historical data and previous solutions to adaptively refine new implementations.

### Advanced Coding Capabilities

- Emergent Intelligence
  - AI autonomously maintains internal state models, supporting continuous refinement.
- Pattern Recognition
  - Autonomous agents perform advanced pattern analysis for effective optimization.
- Adaptive Optimization
  - Continuously evolving feedback loops refine the development process.

### Symbolic Reasoning Integration

- Symbolic Logic Integration
  - Combine symbolic logic with complexity analysis for robust decision-making.
- Information Integration
  - Utilize symbolic mathematics and established software patterns for coherent implementations.
- Coherent Documentation
  - Maintain clear, semantically accurate documentation through symbolic reasoning.

### Code Quality & Style

1. Type Safety Guidelines
   - Use strong typing systems (TypeScript strict mode, Python type hints, Java generics, Rust ownership) and clearly document interfaces, function signatures, and complex logic.

2. Maintainability
   - Write modular, scalable code optimized for clarity and maintenance.

3. Concise Components
   - Keep files concise (under 500 lines) and proactively refactor.

4. Avoid Duplication (DRY)
   - Use symbolic reasoning to systematically identify redundancy.

5. Linting/Formatting
   - Consistently adhere to language-appropriate linting and formatting tools (ESLint/Prettier for JS/TS, Black/flake8 for Python, rustfmt for Rust, gofmt for Go).

6. File Naming
   - Use descriptive, permanent, and standardized naming conventions.

7. No One-Time Scripts
   - Avoid committing temporary utility scripts to production repositories.

### Refactoring

1. Purposeful Changes
   - Refactor with clear objectives: improve readability, reduce redundancy, and meet architecture guidelines.

2. Holistic Approach
   - Consolidate similar components through symbolic analysis.

3. Direct Modification
   - Directly modify existing code rather than duplicating or creating temporary versions.

4. Integration Verification
   - Verify and validate all integrations after changes.
