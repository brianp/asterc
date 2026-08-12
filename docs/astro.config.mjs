// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	integrations: [
		starlight({
			title: 'Aster',
			logo: {
				src: './src/assets/aster.png',
				alt: 'Aster Language',
			},
			favicon: '/favicon.png',
			customCss: ['./src/styles/custom.css'],
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/example/asterc' },
			],
			sidebar: [
				{
					label: 'Getting Started',
					items: [
						{ label: 'Introduction', slug: 'guides/introduction' },
						{ label: 'Installation', slug: 'guides/installation' },
						{ label: 'Hello World', slug: 'guides/hello-world' },
					],
				},
				{
					label: 'Language Basics',
					items: [
						{ label: 'Primitives & Types', slug: 'language/primitives' },
						{ label: 'Variables & Constants', slug: 'language/variables' },
						{ label: 'Operators', slug: 'language/operators' },
						{ label: 'Functions', slug: 'language/functions' },
						{ label: 'Control Flow', slug: 'language/control-flow' },
						{ label: 'Collections', slug: 'language/collections' },
					],
				},
				{
					label: 'Object-Oriented',
					items: [
						{ label: 'Classes', slug: 'oop/classes' },
						{ label: 'Traits', slug: 'oop/traits' },
						{ label: 'Generics', slug: 'oop/generics' },
					],
				},
				{
					label: 'Concurrency',
					items: [
						{ label: 'Overview', slug: 'concurrency/overview' },
						{ label: 'Async & Blocking', slug: 'concurrency/async-blocking' },
						{ label: 'Structured Concurrency', slug: 'concurrency/structured' },
						{ label: 'Mutex, Channels & I/O', slug: 'concurrency/primitives' },
						{ label: 'Green Threads Internals', slug: 'concurrency/green-threads' },
					],
				},
				{
					label: 'Advanced',
					items: [
						{ label: 'Modules & Imports', slug: 'advanced/modules' },
						{ label: 'Error Handling', slug: 'advanced/errors' },
						{ label: 'Pattern Matching', slug: 'advanced/pattern-matching' },
						{ label: 'Type Safety', slug: 'advanced/type-safety' },
					],
				},
			{
					label: 'Tooling',
					items: [
						{ label: 'The asterc CLI', slug: 'tooling/cli' },
						{ label: 'Formatter', slug: 'tooling/formatter' },
						{ label: 'Editor Setup', slug: 'tooling/editors' },
					],
				},
				{
					label: 'Compiler Internals',
					collapsed: true,
					items: [
						{ label: 'Overview', slug: 'internals/overview' },
						{ label: 'Lexer', slug: 'internals/lexer' },
						{ label: 'Parser', slug: 'internals/parser' },
						{ label: 'The AST', slug: 'internals/ast' },
						{ label: 'Type Checker', slug: 'internals/type-checker' },
						{ label: 'The Virtual Standard Library', slug: 'internals/virtual-stdlib' },
						{ label: 'Diagnostics', slug: 'internals/diagnostics' },
						{ label: 'FIR Lowering', slug: 'internals/fir' },
						{ label: 'Code Generation', slug: 'internals/codegen' },
						{ label: 'Contributing', slug: 'internals/contributing' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Glossary', slug: 'reference/glossary' },
						{ label: 'Roadmap & Future Work', slug: 'reference/roadmap' },
					],
				},
				{
					label: 'RFCs',
					items: [
						{ label: 'Language Philosophy', slug: 'rfcs/language-philosophy' },
						{ label: 'Error Handling', slug: 'rfcs/error-handling' },
						{ label: 'Async & Concurrency', slug: 'rfcs/async' },
						{ label: 'Type System', slug: 'rfcs/type-system' },
						{ label: 'Introspection', slug: 'rfcs/introspection' },
						{ label: 'Supervised Tasks', slug: 'rfcs/supervised-tasks' },
						{ label: 'Diagnostics', slug: 'rfcs/diagnostics' },
						{ label: 'Code Generation', slug: 'rfcs/codegen' },
						{ label: 'Formatter', slug: 'rfcs/formatter' },
						{ label: 'LSP Server', slug: 'rfcs/lsp' },
						{ label: 'MCP Server', slug: 'rfcs/mcp-server' },
						{ label: 'REPL', slug: 'rfcs/repl' },
					],
				},
				{
					label: 'Full RFCs',
					collapsed: true,
					items: [
						{ label: 'Language Philosophy', slug: 'rfcs/full/language-philosophy' },
						{ label: 'Error Handling & Nullability', slug: 'rfcs/full/error-handling' },
						{ label: 'Concurrency & Async', slug: 'rfcs/full/async' },
						{ label: 'Type System', slug: 'rfcs/full/type-system' },
						{ label: 'Standard Protocols', slug: 'rfcs/full/protocols' },
						{ label: 'Closure Capture', slug: 'rfcs/full/closures' },
						{ label: 'Introspection', slug: 'rfcs/full/introspection' },
						{ label: 'Supervised Tasks', slug: 'rfcs/full/supervised-tasks' },
						{ label: 'Documentation Comments', slug: 'rfcs/full/doc-comments' },
					],
				},
			],
		}),
	],
});
