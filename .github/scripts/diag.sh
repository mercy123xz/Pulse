for q in "ApolloRed1 Geeked up" "Che RichPhonks" "Drake Gods Plan"; do
  echo "=== $q"; curl -s -w "\nHTTP %{http_code}\n" "https://pulse-accounts.y0hyotuube.workers.dev/tidal/search?q=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$q")" | head -c 2500; echo
done
