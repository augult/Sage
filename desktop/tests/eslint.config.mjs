// Dependency-free browser modules: retain correctness rules without imposing a formatter.
export default [{
  files: ['**/*.js', '**/*.mjs'],
  languageOptions: { ecmaVersion: 'latest', sourceType: 'module' },
  rules: {
    'no-const-assign': 'error', 'no-dupe-args': 'error', 'no-dupe-class-members': 'error',
    'no-dupe-else-if': 'error', 'no-dupe-keys': 'error', 'no-duplicate-case': 'error',
    'no-func-assign': 'error', 'no-unreachable': 'error', 'no-unsafe-finally': 'error',
    'no-unsafe-negation': 'error',
  },
}];
