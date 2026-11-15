# PUBLISH-FORMAT — lib.test.mjs trailing blank line

Scope: `desktop/scripts/portable/test/lib.test.mjs` only. No logic change. Not staged/committed (ROOT re-stages).

## Before
`git diff --cached --check` → `desktop/scripts/portable/test/lib.test.mjs:120: new blank line at EOF.` (rc=2)
File ended `});\n\n` (120 lines), sha256 `d6950fe566e910998ee3d69acd8100673696b93877d1269e0ac857f234f5a18c`.

## Change
Removed the final byte (extra `\n`). File now ends `});\n` (119 lines, exactly one trailing newline),
sha256 `114cb08072b4e9c209e58be0b07944ee78198449bf1f15f54b0c847b3090445d`.

Worktree vs index (`git diff -- desktop/scripts/portable/test/lib.test.mjs`):
```diff
diff --git a/desktop/scripts/portable/test/lib.test.mjs b/desktop/scripts/portable/test/lib.test.mjs
index 7e65351..c2731d2 100644
--- a/desktop/scripts/portable/test/lib.test.mjs
+++ b/desktop/scripts/portable/test/lib.test.mjs
@@ -117,4 +117,3 @@ test('stopChildren forwards to the whole POSIX process group and confirms it is
   assert.deepEqual(alive, []);
   assert.equal(treeAlive(child), false, 'grandchild must be gone too');
 });
-
```

## Checks
- `git diff --check -- <file>` (worktree vs index): no output, rc=0.
- `git diff --no-index --check /dev/null <file>` (simulates new-file staged check): no whitespace errors reported.
- Expected after ROOT `git add`: blob `c2731d2`; `git diff --cached --check` clean for this file.

## Focused test
`cd desktop && node --test scripts/portable/test/lib.test.mjs` (node v25.6.1): tests 9, pass 9, fail 0.
