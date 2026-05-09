@echo off
cd /d "C:\llama.cpp\llama-cpp-turboquant\build\bin\Release"

:: Verificando se o arquivo existe antes de rodar
if not exist llama-server.exe (
    echo [ERRO] O arquivo llama-server.exe nao foi encontrado nesta pasta!
    dir /b *.exe
    pause
    exit
)

start /high /b .\llama-server.exe --embedding -m "J:\Modelos LLM\manifests\registry.ollama.ai\library\Gemma4-26B-A4B\gemma-4-26B-A4B-it-UD-Q4_K_M.gguf" --mmproj "J:\Modelos LLM\manifests\registry.ollama.ai\library\Gemma4-26B-A4B\mmproj-F16.gguf" -ngl 99 --n-cpu-moe 33 --no-mmap -ctk turbo4 -ctv turbo3 -c 16384 --mlock --flash-attn on -t 6 --host 0.0.0.0 --port 8080

pause