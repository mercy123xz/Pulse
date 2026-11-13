for n in pulseplayer pulse-player listentopulse pulsemusic pulse-music pulseapp pulse-app getpulse pulsegg playpulse pulse-gg; do
  echo "$n: $(curl -s -o /dev/null -w '%{http_code}' https://api.github.com/users/$n)"
done
