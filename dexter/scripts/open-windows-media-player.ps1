# Abre o Reprodutor Multimédia do Windows (pacote Groove / Microsoft.ZuneMusic).
# Depois: Biblioteca de músicas → «Ordem aleatória e reproduzir».
$uri = "shell:AppsFolder\Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"
Start-Process -FilePath explorer.exe -ArgumentList $uri
