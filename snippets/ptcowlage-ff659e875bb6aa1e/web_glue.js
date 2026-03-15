// Opens a file dialog with the filter `accept`
export function open_file_dialog(accept) {
    return new Promise((resolve) => {
        const input = document.createElement("input");
        input.type = "file";
        input.accept = accept;
        input.onchange = () => {
            const file = input.files[0];
            const reader = new FileReader();
            reader.onload = () => resolve([file.name, new Uint8Array(reader.result)]);
            reader.readAsArrayBuffer(file);
        };
        input.click();
    });
}

export function save_file(data, filename) {
    const blob = new Blob([data]);
    const url = URL.createObjectURL(blob);

    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    a.click();

    URL.revokeObjectURL(url);
}
