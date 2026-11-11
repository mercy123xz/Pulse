# The headphones at the top of the page

`headphones.src.js` builds the headphones in code with three.js. The page loads the
bundled copy, `../headphones.js`, after everything else, and shows the still picture
`../img/headphones.webp` until then (and for good where WebGL isn't available).

To rebuild after a change:

    npm install three esbuild
    npx esbuild headphones.src.js --bundle --format=esm --minify --target=es2020 \
      --legal-comments=eof --outfile=../headphones.js

The still picture is the model's resting pose: `mount(canvas).snapshot()` on a 900 by 900
canvas returns it as an image.
